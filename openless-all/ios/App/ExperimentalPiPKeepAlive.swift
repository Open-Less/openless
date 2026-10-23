import AVKit
import Foundation
import SwiftUI
import UIKit

/// The source stays attached during the activation sheet and keyboard round trip.
/// It is transparent content, not a hidden/detached view (AVKit needs its geometry).
final class PiPSourceView: UIView {
  var onAttach: (() -> Void)?
  override func didMoveToWindow() {
    super.didMoveToWindow()
    if window != nil { onAttach?() }
  }
}

struct PiPInlineSource: UIViewRepresentable {
  let keepAlive: ExperimentalPiPKeepAlive
  func makeUIView(context: Context) -> PiPSourceView { keepAlive.sourceView }
  func updateUIView(_ uiView: PiPSourceView, context: Context) {}
}

/// Content-only VideoCall PiP: no player, input node or output engine.
/// The private compatibility layer is isolated and can decline unsupported systems.
@MainActor
final class ExperimentalPiPKeepAlive: NSObject {
  let sourceView = PiPSourceView()
  private var controller: AVPictureInPictureController?
  private var contentController: AVPictureInPictureVideoCallViewController?
  private let coexistence = PiPCoexistenceAdapter()
  private var observations: [NSKeyValueObservation] = []
  private var notifications: [NSObjectProtocol] = []
  private var retryTask: Task<Void, Never>?
  private var startTimeout: Task<Void, Never>?
  private var lastError: String?
  private var enabled = false
  private var starting = false
  private var protectedDataUnavailable = false
  private var lockRecovery = PiPLockRecovery()
  private var audioPrepared = false
  private var inForeground = UIApplication.shared.applicationState == .active
  private var foregroundStopRecoveryUsed = false
  private var contentTimer: Timer?
  private var contentPhase = false
  private var generation = UUID()
  var onChange: (() -> Void)?
  /// The audio owner may prepare a short playback session; PiP never sets category/routes.
  var onPrepareStart: (() throws -> Void)?
  /// Called exactly once for each successful prepare, including cancellation/timeout.
  var onSettled: (() -> Void)?

  var isImplemented: Bool { true } // Deployment target is iOS 16; VideoCall requires iOS 15.
  var isSupported: Bool { AVPictureInPictureController.isPictureInPictureSupported() }
  var isActive: Bool { controller?.isPictureInPictureActive == true }
  var isRecovering: Bool { enabled && (starting || retryTask != nil) }
  var isReady: Bool {
    enabled && isSupported && isActive && controller?.isPictureInPictureSuspended == false
      && !protectedDataUnavailable && UIApplication.shared.isProtectedDataAvailable
  }

  var explanation: String {
    if !enabled { return "免跳转后台保活已关闭。" }
    if isReady { return "免跳转后台保活已激活；待机时不采集麦克风、不播放音频。" }
    if protectedDataUnavailable { return "设备已锁定；解锁后恢复免跳转状态。" }
    if let lastError { return "PiP 保活暂不可用：\(lastError)" }
    return "正在准备免跳转后台保活；待机时不采集麦克风。"
  }

  var diagnosticReport: String {
    "route=hiddenVideoCall height=0.1 supported=\(isSupported) active=\(isActive) ready=\(isReady) "
      + "suspended=\(controller?.isPictureInPictureSuspended == true) enabled=\(enabled) "
      + "possible=\(controller?.isPictureInPicturePossible == true) attached=\(sourceView.window != nil) "
      + "starting=\(starting) recovering=\(isRecovering) protected=\(protectedDataUnavailable) "
      + "\(coexistence.diagnosticReport) error=\(lastError ?? "none") "
      + "appState=\(UIApplication.shared.applicationState.rawValue)"
  }

  override init() {
    super.init()
    sourceView.backgroundColor = .clear
    sourceView.isOpaque = false
    sourceView.isUserInteractionEnabled = false
    sourceView.accessibilityElementsHidden = true
    sourceView.onAttach = { [weak self] in
      Task { @MainActor [weak self] in
        guard let self, self.enabled else { return }
        self.makeControllerIfNeeded()
        if self.retryTask != nil { self.attemptStart() }
      }
    }
    for name in [UIApplication.protectedDataWillBecomeUnavailableNotification,
                 UIApplication.protectedDataDidBecomeAvailableNotification] {
      notifications.append(NotificationCenter.default.addObserver(forName: name, object: nil, queue: .main) {
        [weak self] notification in
        let locked = notification.name == UIApplication.protectedDataWillBecomeUnavailableNotification
        Task { @MainActor [weak self] in self?.protectedDataChanged(locked: locked) }
      })
    }
  }

  func arm() {
    guard !enabled else { return }
    enabled = true
    lockRecovery.reset()
    lastError = nil
    protectedDataUnavailable = !UIApplication.shared.isProtectedDataAvailable
    makeControllerIfNeeded()
    onChange?()
  }

  func startIfPossible() {
    guard enabled, isSupported, !isActive, !isRecovering,
      !protectedDataUnavailable, UIApplication.shared.isProtectedDataAvailable else { return }
    lastError = nil
    guard makeControllerIfNeeded() else { onChange?(); return }
    // A single bounded attempt includes waiting for source layout and AVKit eligibility.
    let token = generation
    retryTask = Task { @MainActor [weak self] in
      for _ in 0..<20 {
        guard !Task.isCancelled, let self, self.enabled, self.generation == token,
          !self.protectedDataUnavailable else { return }
        self.attemptStart()
        if self.starting || self.isActive { return }
        try? await Task.sleep(for: .milliseconds(250))
      }
      guard !Task.isCancelled, let self, self.generation == token else { return }
      self.retryTask = nil
      self.lastError = "系统尚未允许 PiP；返回主应用可重试"
      self.settleAudio()
      self.onChange?()
    }
    onChange?()
  }

  /// Resume after a genuine foreground visit. An involuntary background stop
  /// reports unavailable instead of repeatedly competing with another app's PiP.
  func foregroundChanged(_ foreground: Bool) {
    if foreground && !inForeground { foregroundStopRecoveryUsed = false }
    inForeground = foreground
    guard enabled else { return }
    if foreground {
      protectedDataUnavailable = !UIApplication.shared.isProtectedDataAvailable
      if isActive { scheduleCoexistenceTransition() }
      else {
        if !isRecovering, controller != nil { disposeController(stop: true) }
        startIfPossible()
      }
    } else if UIApplication.shared.applicationState != .background {
      startIfPossible()
    }
    onChange?()
  }

  func stop() {
    enabled = false
    lockRecovery.reset()
    disposeController(stop: true)
    lastError = nil
    onChange?()
  }

  func collectDiagnostics() -> String {
    diagnosticReport + "\n透明 VideoCall/0.1pt；共存适配使用经过签名检查的系统内部接口。"
      + "提交成功不等于系统已接受多 PiP；不支持时免跳转不可用。"
  }

  @discardableResult
  private func makeControllerIfNeeded() -> Bool {
    if controller != nil { return true }
    guard enabled, isSupported else { lastError = "当前设备不支持画中画"; return false }
    let content = AVPictureInPictureVideoCallViewController()
    content.preferredContentSize = CGSize(width: 300, height: 0.1)
    content.view.frame = CGRect(origin: .zero, size: content.preferredContentSize)
    content.view.backgroundColor = .clear
    content.view.isOpaque = false
    content.view.layer.isOpaque = false
    content.view.clipsToBounds = true
    let source = AVPictureInPictureController.ContentSource(
      activeVideoCallSourceView: sourceView, contentViewController: content)
    guard let candidate = PiPHiddenControls.makeController(source: source),
      coexistence.prepare(controller: candidate) else {
      coexistence.restore()
      lastError = "当前系统的隐藏画中画接口不兼容；请使用返回主应用录音"
      return false
    }
    candidate.delegate = self
    candidate.requiresLinearPlayback = true
    // Explicit start owns the short audio preparation lease and bounded timeout.
    candidate.canStartPictureInPictureAutomaticallyFromInline = false
    controller = candidate
    contentController = content
    observations = [candidate.observe(\.isPictureInPictureSuspended, options: [.new]) { [weak self] candidate, _ in
      let identity = ObjectIdentifier(candidate)
      Task { @MainActor [weak self] in
        guard let self, let candidate = self.controller, ObjectIdentifier(candidate) == identity else { return }
        if !candidate.isPictureInPictureSuspended { self.scheduleCoexistenceTransition() }
        self.updateContentRefresh()
        self.onChange?()
      }
    }, candidate.observe(\.isPictureInPictureActive, options: [.new]) { [weak self] candidate, _ in
      let identity = ObjectIdentifier(candidate)
      Task { @MainActor [weak self] in
        guard let self, let candidate = self.controller, ObjectIdentifier(candidate) == identity else { return }
        self.updateContentRefresh()
        self.onChange?()
      }
    }]
    return true
  }

  private func attemptStart() {
    guard enabled, !starting, !isActive, !protectedDataUnavailable,
      UIApplication.shared.isProtectedDataAvailable, let controller,
      sourceView.window != nil, !sourceView.bounds.isEmpty else { return }
    if !audioPrepared {
      do { try onPrepareStart?(); audioPrepared = true }
      catch {
        retryTask?.cancel(); retryTask = nil
        lastError = error.localizedDescription
        onChange?()
        return
      }
    }
    guard controller.isPictureInPicturePossible else { return }
    retryTask?.cancel(); retryTask = nil
    starting = true
    let token = generation
    startTimeout = Task { @MainActor [weak self] in
      try? await Task.sleep(for: .seconds(5))
      guard !Task.isCancelled, let self, self.generation == token, self.starting else { return }
      self.lastError = "系统未完成 PiP 启动；返回主应用可重试"
      self.disposeController(stop: true)
      self.onChange?()
    }
    NSLog("OPENLESS_PIP_START %@", diagnosticReport)
    controller.startPictureInPicture()
    onChange?()
  }

  private func scheduleCoexistenceTransition() {
    guard controller != nil else { return }
    let token = generation
    coexistence.scheduleTransition { [weak self] in
      guard Thread.isMainThread else { return false }
      return MainActor.assumeIsolated {
        guard let self, self.generation == token, let candidate = self.controller else { return false }
        return self.enabled && candidate.isPictureInPictureActive
          && !candidate.isPictureInPictureSuspended && !self.protectedDataUnavailable
      }
    }
  }

  private func protectedDataChanged(locked: Bool) {
    protectedDataUnavailable = locked
    updateContentRefresh()
    if locked {
      lockRecovery.didLock(generation: generation, active: isActive, recovering: isRecovering)
      retryTask?.cancel(); retryTask = nil
      if starting { disposeController(stop: true) }
      settleAudio()
    } else {
      let recoverLockedSession = lockRecovery.consumeOnUnlock(generation: generation, active: isActive)
      if enabled {
        if isActive { scheduleCoexistenceTransition() }
        else if recoverLockedSession || UIApplication.shared.applicationState == .active {
          // Consume before starting: failure does not create an unlock retry loop.
          // Only a stopped/interrupted locked session may restart in background.
          if !isRecovering, controller != nil { disposeController(stop: true) }
          startIfPossible()
        }
      }
    }
    onChange?()
  }

  /// Refresh content once per second while PiP is active and unsuspended.
  private func updateContentRefresh() {
    let eligible = enabled && isActive && controller?.isPictureInPictureSuspended == false
      && !protectedDataUnavailable
    guard eligible else { contentTimer?.invalidate(); contentTimer = nil; return }
    guard contentTimer == nil else { return }
    let timer = Timer(timeInterval: 1, repeats: true) { [weak self] _ in
      Task { @MainActor [weak self] in
        guard let self, self.isReady else { return }
        self.contentPhase.toggle()
        // A transparent layer update avoids starting an audio or video engine.
        self.contentController?.view.layer.backgroundColor = UIColor(white: 0,
          alpha: self.contentPhase ? 0.001 : 0).cgColor
        self.contentController?.view.setNeedsDisplay()
      }
    }
    contentTimer = timer
    RunLoop.main.add(timer, forMode: .common)
  }

  private func settleAudio() {
    guard audioPrepared else { return }
    audioPrepared = false
    onSettled?()
  }

  private func disposeController(stop: Bool) {
    generation = UUID()
    retryTask?.cancel(); retryTask = nil
    startTimeout?.cancel(); startTimeout = nil
    starting = false
    contentTimer?.invalidate(); contentTimer = nil
    observations.removeAll()
    let old = controller
    controller = nil
    old?.delegate = nil
    old?.canStartPictureInPictureAutomaticallyFromInline = false
    if stop { old?.stopPictureInPicture() }
    coexistence.restore()
    contentController = nil
    settleAudio()
  }

  private func handleStarted(_ identity: ObjectIdentifier) {
    guard let candidate = controller, ObjectIdentifier(candidate) == identity, enabled else { return }
    starting = false
    retryTask?.cancel(); retryTask = nil
    startTimeout?.cancel(); startTimeout = nil
    lastError = nil
    scheduleCoexistenceTransition()
    updateContentRefresh()
    settleAudio()
    NSLog("OPENLESS_PIP_STARTED %@", diagnosticReport)
    onChange?()
  }

  private func handleStopped(_ identity: ObjectIdentifier, error: Error? = nil) {
    guard let candidate = controller, ObjectIdentifier(candidate) == identity else { return }
    if enabled { lockRecovery.didStop(generation: generation) }
    lastError = error?.localizedDescription ?? "画中画已结束；返回主应用可恢复免跳转"
    disposeController(stop: false)
    NSLog("OPENLESS_PIP_STOPPED %@", diagnosticReport)
    onChange?()
    // didStop can arrive after the foreground notification. Recover at most
    // once per foreground visit, and never restart a displaced background PiP.
    if error == nil, enabled, UIApplication.shared.applicationState == .active,
      !foregroundStopRecoveryUsed {
      foregroundStopRecoveryUsed = true
      startIfPossible()
    }
  }

  isolated deinit {
    contentTimer?.invalidate()
    notifications.forEach(NotificationCenter.default.removeObserver)
  }
}

extension ExperimentalPiPKeepAlive: AVPictureInPictureControllerDelegate {
  nonisolated func pictureInPictureControllerDidStartPictureInPicture(_ controller: AVPictureInPictureController) {
    let identity = ObjectIdentifier(controller)
    Task { @MainActor [weak self] in self?.handleStarted(identity) }
  }
  nonisolated func pictureInPictureControllerDidStopPictureInPicture(_ controller: AVPictureInPictureController) {
    let identity = ObjectIdentifier(controller)
    Task { @MainActor [weak self] in self?.handleStopped(identity) }
  }
  nonisolated func pictureInPictureController(_ controller: AVPictureInPictureController,
    failedToStartPictureInPictureWithError error: Error) {
    let identity = ObjectIdentifier(controller)
    Task { @MainActor [weak self] in self?.handleStopped(identity, error: error) }
  }
  nonisolated func pictureInPictureController(_ controller: AVPictureInPictureController,
    restoreUserInterfaceForPictureInPictureStopWithCompletionHandler completionHandler: @escaping (Bool) -> Void) {
    completionHandler(false)
  }
}
