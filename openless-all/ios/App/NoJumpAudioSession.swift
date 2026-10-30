import AVFoundation
import Foundation
import UIKit

/// Owns audio only while recording, playing history, or starting PiP. Idle
/// eligibility belongs to the live PiP controller, never a silent render loop.
@MainActor
final class NoJumpAudioSession {
  enum CaptureAdmission { case foreground, system, keyboard(UUID) }
  static let shared = NoJumpAudioSession()

  private let driver: any KeyboardAudioSessionDriver
  private let permission: () -> Bool
  private let unlocked: () -> Bool
  private let foreground: () -> Bool
  private var observers: [NSObjectProtocol] = []
  private var activationGeneration: UUID?
  private var captureToken: UUID?
  private var playbackToken: UUID?
  private var releaseTask: Task<Void, Never>?
  private var releaseRevision = UUID()
  private var preparingPiP = false
  private var systemInterrupted = false
  private var protectedDataUnavailable = false
  private var mediaServicesAvailable = true
  private(set) var ownsAudioSession = false
  var isBackgroundCaptureAvailable: () -> Bool = { false }
  var onChange: (() -> Void)?
  var onInterruption: (() -> Void)?

  var hasActivationIntent: Bool { activationGeneration != nil }
  var isReady: Bool {
    hasActivationIntent && permission() && unlocked() && !protectedDataUnavailable
      && !systemInterrupted && mediaServicesAvailable && playbackToken == nil
      && !(ownsAudioSession && captureToken == nil && !preparingPiP)
      && (foreground() || isBackgroundCaptureAvailable())
  }
  var generation: UUID? { isReady ? activationGeneration : nil }
  var diagnosticReport: String {
    "ready=\(isReady) activation=\(activationGeneration?.uuidString ?? "none") "
      + "pip=\(isBackgroundCaptureAvailable()) idleAudio=\(ownsAudioSession && captureToken == nil && playbackToken == nil) "
      + "capturing=\(captureToken != nil) playback=\(playbackToken != nil) preparingPiP=\(preparingPiP) "
      + "interrupted=\(systemInterrupted) unlocked=\(unlocked() && !protectedDataUnavailable) "
      + "mediaServices=\(mediaServicesAvailable) foreground=\(foreground()) \(driver.diagnosticReport)"
  }

  init(driver: any KeyboardAudioSessionDriver = SystemKeyboardAudioSessionDriver(),
    permission: @escaping () -> Bool = { MicrophoneAccess.isGranted },
    unlocked: @escaping () -> Bool = { UIApplication.shared.isProtectedDataAvailable },
    foreground: @escaping () -> Bool = { UIApplication.shared.applicationState == .active },
    observeSystem: Bool = true) {
    self.driver = driver; self.permission = permission
    self.unlocked = unlocked; self.foreground = foreground
    if observeSystem { observeNotifications() }
  }

  func activate() async throws {
    guard foreground(), permission(), unlocked(), !protectedDataUnavailable,
      !systemInterrupted, mediaServicesAvailable, captureToken == nil, playbackToken == nil else {
      throw VoiceFailure.cancelled
    }
    if activationGeneration == nil { activationGeneration = UUID() }
    // No AVAudioEngine, input node, render timer or active audio session here.
    NSLog("OPENLESS_AUDIO_ARM %@", diagnosticReport)
    onChange?()
  }

  func admits(_ generation: UUID?) -> Bool {
    generation != nil && generation == activationGeneration && isReady
  }

  func deactivate() {
    activationGeneration = nil
    preparingPiP = false
    releaseIdleAudio()
    onChange?()
  }

  func deactivate(generation: UUID) {
    guard activationGeneration == generation else { return }
    deactivate()
  }

  /// AVKit may need a playback session during startup, but it must not leave
  /// the route in playAndRecord or keep audio active after PiP has started.
  func preparePiPStart() throws {
    guard hasActivationIntent, permission(), unlocked(), !protectedDataUnavailable,
      !systemInterrupted, mediaServicesAvailable else { throw VoiceFailure.cancelled }
    guard captureToken == nil, playbackToken == nil else { return }
    cancelIdleRelease()
    do {
      try driver.preparePiP()
      ownsAudioSession = true
      preparingPiP = true
    } catch {
      // setCategory may have succeeded before setActive failed.
      ownsAudioSession = true
      releaseIdleAudio()
      throw error
    }
  }

  func settlePiPStart() {
    preparingPiP = false
    releaseIdleAudio()
    onChange?()
  }

  func backgroundCapabilityDidChange() {
    // Capability changes affect the next request; they do not restart a mic.
    onChange?()
  }

  func foregroundChanged() {
    if !preparingPiP { releaseIdleAudio() }
    onChange?()
  }

  func acquireCapture(admission: CaptureAdmission) throws -> UUID {
    guard captureToken == nil, playbackToken == nil, permission(),
      !systemInterrupted, mediaServicesAvailable else {
      throw VoiceFailure.cancelled
    }
    switch admission {
    case .keyboard(let generation):
      guard admits(generation), isBackgroundCaptureAvailable() else { throw VoiceFailure.cancelled }
    case .foreground:
      guard foreground() else { throw VoiceFailure.cancelled }
    case .system: break // AppController separately holds system recording admission.
    }
    cancelIdleRelease()
    do {
      try driver.configureCapture()
      ownsAudioSession = true
      preparingPiP = false
      let token = UUID(); captureToken = token
      NSLog("OPENLESS_AUDIO_CAPTURE_ACQUIRE %@", diagnosticReport)
      return token
    } catch {
      ownsAudioSession = true
      releaseIdleAudio()
      throw error
    }
  }

  func releaseCapture(_ token: UUID) {
    guard captureToken == token else { return }
    captureToken = nil
    preparingPiP = false
    releaseIdleAudio()
    NSLog("OPENLESS_AUDIO_CAPTURE_RELEASE %@", diagnosticReport)
    onChange?()
  }

  func acquirePlayback() throws -> UUID {
    guard captureToken == nil, playbackToken == nil, !systemInterrupted,
      mediaServicesAvailable else { throw VoiceFailure.cancelled }
    cancelIdleRelease()
    do {
      try driver.configurePlayback()
      ownsAudioSession = true
      preparingPiP = false
      let token = UUID(); playbackToken = token
      onChange?()
      return token
    } catch {
      ownsAudioSession = true
      releaseIdleAudio()
      throw error
    }
  }

  func releasePlayback(_ token: UUID) {
    guard playbackToken == token else { return }
    playbackToken = nil
    releaseIdleAudio()
    onChange?()
  }

  private func releaseIdleAudio(scheduleRetry: Bool = true) {
    guard captureToken == nil, playbackToken == nil, !preparingPiP, ownsAudioSession else { return }
    do {
      try driver.release()
      ownsAudioSession = false
      cancelIdleRelease()
      NSLog("OPENLESS_AUDIO_IDLE_RELEASED %@", diagnosticReport)
    } catch {
      // Keep the ownership flag so the next release retries; never report a
      // failed release as successful or repeatedly reactivate a receiver route.
      NSLog("OPENLESS_AUDIO_RELEASE_FAILED error=%@ %@", error.localizedDescription, diagnosticReport)
      if scheduleRetry { scheduleIdleRelease() }
    }
  }

  private func cancelIdleRelease() {
    releaseRevision = UUID()
    releaseTask?.cancel(); releaseTask = nil
  }

  private func scheduleIdleRelease() {
    guard releaseTask == nil else { return }
    let revision = releaseRevision
    releaseTask = Task { [weak self] in
      defer { if self?.releaseRevision == revision { self?.releaseTask = nil } }
      for delay in [100, 300, 900] {
        do { try await Task.sleep(for: .milliseconds(delay)) } catch { return }
        guard let self, self.releaseRevision == revision, self.captureToken == nil,
          self.playbackToken == nil, !self.preparingPiP, self.ownsAudioSession else { return }
        self.releaseIdleAudio(scheduleRetry: false)
        self.onChange?()
        if !self.ownsAudioSession { return }
      }
    }
  }

  private func observeNotifications() {
    var names: [Notification.Name] = [AVAudioSession.interruptionNotification,
      AVAudioSession.routeChangeNotification, AVAudioSession.mediaServicesWereLostNotification,
      AVAudioSession.mediaServicesWereResetNotification, UIApplication.protectedDataWillBecomeUnavailableNotification,
      UIApplication.protectedDataDidBecomeAvailableNotification]
    if #available(iOS 27, *) { names.append(AVAudioSession.didBecomeInactiveNotification) }
    for name in names {
      observers.append(NotificationCenter.default.addObserver(forName: name, object: nil, queue: .main) {
        [weak self] notification in
        let eventName = notification.name
        let type = notification.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt
        let route = notification.userInfo?[AVAudioSessionRouteChangeReasonKey] as? UInt
        var appDeactivation = false
        if #available(iOS 27, *),
          let context = notification.userInfo?[AVAudioSession.deactivationContextKey] as? AVAudioSession.DeactivationContext {
          appDeactivation = context.source == .app
        }
        MainActor.assumeIsolated {
          self?.handleEvent(name: eventName, interruption: type, route: route, appDeactivation: appDeactivation)
        }
      })
    }
  }

  // Internal so tests can exercise the same notification ordering without
  // changing the user's audio devices or posting global notifications.
  func handle(_ notification: Notification) {
    var appDeactivation = false
    if #available(iOS 27, *),
      let context = notification.userInfo?[AVAudioSession.deactivationContextKey] as? AVAudioSession.DeactivationContext {
      appDeactivation = context.source == .app
    }
    handleEvent(name: notification.name,
      interruption: notification.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt,
      route: notification.userInfo?[AVAudioSessionRouteChangeReasonKey] as? UInt,
      appDeactivation: appDeactivation)
  }

  private func handleEvent(name: Notification.Name, interruption: UInt?, route: UInt?, appDeactivation: Bool) {
    NSLog("OPENLESS_AUDIO_EVENT name=%@ %@", name.rawValue, diagnosticReport)
    if name == UIApplication.protectedDataWillBecomeUnavailableNotification {
      protectedDataUnavailable = true
      interruptCapture()
    } else if name == UIApplication.protectedDataDidBecomeAvailableNotification {
      protectedDataUnavailable = false
    } else if name == AVAudioSession.mediaServicesWereLostNotification {
      mediaServicesAvailable = false
      interruptCapture()
    } else if name == AVAudioSession.mediaServicesWereResetNotification {
      interruptCapture()
      mediaServicesAvailable = true
      systemInterrupted = false
      ownsAudioSession = false
    } else if name == AVAudioSession.interruptionNotification {
      let type = interruption
      if type == AVAudioSession.InterruptionType.began.rawValue {
        systemInterrupted = true
        interruptCapture()
      } else if type == AVAudioSession.InterruptionType.ended.rawValue {
        systemInterrupted = false
        // No automatic playback or capture: shouldResume is irrelevant to a
        // fresh, explicit keyboard request after this interruption has ended.
      }
    } else if name == AVAudioSession.routeChangeNotification {
      let reason = route
      if [AVAudioSession.RouteChangeReason.oldDeviceUnavailable.rawValue,
        AVAudioSession.RouteChangeReason.newDeviceAvailable.rawValue,
        AVAudioSession.RouteChangeReason.noSuitableRouteForCategory.rawValue].contains(reason ?? 0) {
        // Idle output selection in another app must never reactivate our audio.
        interruptCapture()
      }
    } else if #available(iOS 27, *), name == AVAudioSession.didBecomeInactiveNotification {
      if appDeactivation { return }
      ownsAudioSession = false
      interruptCapture()
    }
    if !preparingPiP { releaseIdleAudio() }
    onChange?()
  }

  private func interruptCapture() {
    preparingPiP = false
    if captureToken != nil { onInterruption?() }
  }
}

@MainActor
protocol KeyboardAudioSessionDriver {
  var diagnosticReport: String { get }
  func preparePiP() throws
  func configureCapture() throws
  func configurePlayback() throws
  func release() throws
}

@MainActor
struct SystemKeyboardAudioSessionDriver: KeyboardAudioSessionDriver {
  private var session: AVAudioSession { .sharedInstance() }
  var diagnosticReport: String {
    "category=\(session.category.rawValue) mode=\(session.mode.rawValue) options=\(session.categoryOptions.rawValue) "
      + "routeIn=\(session.currentRoute.inputs.map { $0.portType.rawValue }.joined(separator: ",")) "
      + "routeOut=\(session.currentRoute.outputs.map { $0.portType.rawValue }.joined(separator: ",")) volume=\(session.outputVolume)"
  }
  func preparePiP() throws {
    try session.setCategory(.playback, mode: .default, options: [.mixWithOthers])
    try session.setActive(true)
  }
  func configureCapture() throws {
    let bluetooth: AVAudioSession.CategoryOptions
    if #available(iOS 26, *) { bluetooth = .allowBluetoothHFP }
    else { bluetooth = .allowBluetooth }
    try session.setCategory(.playAndRecord, mode: .measurement,
      options: [.mixWithOthers, .defaultToSpeaker, bluetooth])
    try session.setActive(true)
  }
  func configurePlayback() throws {
    try session.setCategory(.playback, mode: .spokenAudio, options: [.duckOthers])
    try session.setActive(true)
  }
  func release() throws {
    try session.setActive(false, options: .notifyOthersOnDeactivation)
    // Remove receiver/HFP category semantics while retaining PiP eligibility.
    // Do not call overrideOutputAudioPort: the user's route stays theirs.
    try session.setCategory(.playback, mode: .default, options: [.mixWithOthers])
  }
}
