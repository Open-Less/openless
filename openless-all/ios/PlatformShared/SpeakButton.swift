import OpenLessShared
import UIKit

/// Shared UIKit control so the main-app trial and keyboard use identical touch
/// arbitration. Layout changes never replace the currently tracked control.
final class SpeakButton: UIControl {
  var onAction: ((VoiceGesture.Action) -> Void)?
  var onUnavailable: (() -> Void)?
  enum TranslationTouch { case began, moved, ended, cancelled }
  /// Points are in this control's coordinates. The owner keeps the control
  /// attached while the arc takes over drawing and resolves the terminal hit.
  var onTranslationTouch: ((TranslationTouch, CGPoint) -> Bool)?
  private var translationGesture = VoiceTranslationGesture()
  private var pressOrigin = CGPoint.zero
  private var pressLocation = CGPoint.zero
  var isSelectingTranslation: Bool { translationGesture.isSelecting }
  /// Optional idle-action tint, independent of recording/processing colors.
  var unavailableTitleColor: UIColor?
  var interfaceLanguage: InterfaceLanguage = .system
  private var unavailablePress = false
  enum Presentation { case standard, video, helpWrite }
  var presentation: Presentation = .standard
  /// The owner supplies page visibility; off-page controls must not keep animating.
  var presentationIsVisible = true { didSet { updateProcessingAnimation() } }
  private var renderedPresentation: Presentation = .standard
  private let processingSweep = CAGradientLayer()
  private var applicationIsActive = true
  private let ring = UIView()
  private let shape = UIView()
  private let image = UIImageView(image: UIImage(systemName: "mic.fill"))
  private let idleTitle = UILabel()
  private let idleContent = UIStackView()
  private let label = UILabel()
  private let waves = UIStackView()
  private var bars: [UIView] = []
  private var barHeights: [NSLayoutConstraint] = []
  private var width: NSLayoutConstraint!
  private var height: NSLayoutConstraint!
  private var imageWidth: NSLayoutConstraint!
  private var imageHeight: NSLayoutConstraint!
  private var phase: VoicePhase = .inactive
  private var available = false
  private var compact = false
  private var pressInside = true
  private var gesture = VoiceGesture()
  private var holdTimer: Timer?
  private var animator: UIViewPropertyAnimator?
  private var levels = [CGFloat](repeating: 0, count: 7)

  override init(frame: CGRect) {
    super.init(frame: frame)
    isAccessibilityElement = true
    accessibilityTraits = .button
    accessibilityIdentifier = "speakButton"
    shape.isUserInteractionEnabled = false
    shape.translatesAutoresizingMaskIntoConstraints = false
    shape.layer.cornerRadius = 30
    shape.clipsToBounds = true
    processingSweep.startPoint = CGPoint(x: 0, y: 0.5)
    processingSweep.endPoint = CGPoint(x: 1, y: 0.5)
    processingSweep.isHidden = true
    shape.layer.addSublayer(processingSweep)
    NotificationCenter.default.addObserver(self, selector: #selector(reduceMotionChanged),
      name: UIAccessibility.reduceMotionStatusDidChangeNotification, object: nil)
    for name in [UIWindow.didBecomeVisibleNotification, UIWindow.didBecomeHiddenNotification] {
      NotificationCenter.default.addObserver(self, selector: #selector(windowVisibilityChanged(_:)),
        name: name, object: nil)
    }
    NotificationCenter.default.addObserver(self, selector: #selector(applicationWillResignActive),
      name: UIApplication.willResignActiveNotification, object: nil)
    NotificationCenter.default.addObserver(self, selector: #selector(applicationDidBecomeActive),
      name: UIApplication.didBecomeActiveNotification, object: nil)
    ring.isUserInteractionEnabled = false
    ring.translatesAutoresizingMaskIntoConstraints = false
    ring.backgroundColor = UIColor.label.withAlphaComponent(0.08)
    addSubview(ring)
    addSubview(shape)
    width = shape.widthAnchor.constraint(equalToConstant: 147)
    height = shape.heightAnchor.constraint(equalToConstant: 60)
    NSLayoutConstraint.activate([
      width, height, shape.centerXAnchor.constraint(equalTo: centerXAnchor),
      shape.centerYAnchor.constraint(equalTo: centerYAnchor),
      ring.centerXAnchor.constraint(equalTo: shape.centerXAnchor),
      ring.centerYAnchor.constraint(equalTo: shape.centerYAnchor),
      ring.widthAnchor.constraint(equalTo: shape.widthAnchor, constant: 24),
      ring.heightAnchor.constraint(equalTo: shape.heightAnchor, constant: 24),
    ])
    image.translatesAutoresizingMaskIntoConstraints = false
    image.contentMode = .scaleAspectFit
    idleTitle.text = "开始说话"
    idleTitle.font = .systemFont(ofSize: 14, weight: .semibold)
    idleContent.axis = .horizontal
    idleContent.alignment = .center
    idleContent.spacing = 8
    idleContent.translatesAutoresizingMaskIntoConstraints = false
    idleContent.addArrangedSubview(image)
    idleContent.addArrangedSubview(idleTitle)
    shape.addSubview(idleContent)
    label.font = .systemFont(ofSize: 14, weight: .medium)
    label.textAlignment = .center
    label.translatesAutoresizingMaskIntoConstraints = false
    shape.addSubview(label)
    waves.axis = .horizontal
    waves.alignment = .center
    waves.spacing = 4
    waves.translatesAutoresizingMaskIntoConstraints = false
    shape.addSubview(waves)
    for _ in 0..<7 {
      let bar = UIView()
      bar.layer.cornerRadius = 2
      let barHeight = bar.heightAnchor.constraint(equalToConstant: 4)
      bar.widthAnchor.constraint(equalToConstant: 5).isActive = true
      barHeight.isActive = true
      bars.append(bar)
      barHeights.append(barHeight)
      waves.addArrangedSubview(bar)
    }
    imageWidth = image.widthAnchor.constraint(equalToConstant: 20)
    imageHeight = image.heightAnchor.constraint(equalToConstant: 26)
    NSLayoutConstraint.activate([
      idleContent.centerXAnchor.constraint(equalTo: shape.centerXAnchor),
      idleContent.centerYAnchor.constraint(equalTo: shape.centerYAnchor),
      imageWidth, imageHeight,
      label.leadingAnchor.constraint(equalTo: shape.leadingAnchor, constant: 8),
      label.trailingAnchor.constraint(equalTo: shape.trailingAnchor, constant: -8),
      label.centerYAnchor.constraint(equalTo: shape.centerYAnchor),
      waves.centerXAnchor.constraint(equalTo: shape.centerXAnchor),
      waves.centerYAnchor.constraint(equalTo: shape.centerYAnchor),
      waves.heightAnchor.constraint(equalToConstant: 54),
    ])
    configure(phase: .inactive, level: 0, available: false)
  }
  convenience init(compact: Bool) {
    self.init(frame: .zero)
    self.compact = compact
    if compact {
      width.constant = 38
      height.constant = 38
      shape.layer.cornerRadius = 19
      idleTitle.isHidden = true
      waves.transform = CGAffineTransform(scaleX: 0.5, y: 0.5)
      label.font = .systemFont(ofSize: 10, weight: .medium)
      label.adjustsFontSizeToFitWidth = true
      label.minimumScaleFactor = 0.5
    }
  }
  required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

  func configure(phase: VoicePhase, level: Float, available: Bool, idleLabel: String = "开始说话") {
    let changed = self.phase != phase
    let presentationChanged = renderedPresentation != presentation
    renderedPresentation = presentation
    let level = level.isFinite ? min(1, max(0, level)) : 0
    if changed { levels = [CGFloat](repeating: 0, count: 7) }
    let localizer = InterfaceLocalizer(language: interfaceLanguage)
    idleTitle.text = localizer.text(idleLabel)
    idleTitle.isHidden = compact || presentation != .standard
    waves.transform = compact ? CGAffineTransform(scaleX: 0.5, y: 0.5)
      : presentation == .helpWrite ? CGAffineTransform(scaleX: 0.55, y: 0.55) : .identity
    imageWidth.constant = presentation == .video ? 28 : 20
    imageHeight.constant = presentation == .video ? 36 : 26
    self.phase = phase
    self.available = available
    let busy = phase == .processing || phase == .awaitingInsertion || phase == .starting
    let recording = phase == .recording
    idleContent.isHidden = busy || recording
    label.isHidden = !busy
    waves.isHidden = !recording
    label.text = localizer.text(phase == .starting ? "准备中…" : phase == .awaitingInsertion ? "等待插入" : "thinking")
    ring.isHidden = presentation == .standard || !recording
    let amplitude = CGFloat(VoiceWaveform.amplitude(rms: level))
    let ringScale = UIAccessibility.isReduceMotionEnabled ? 1 : 1 + amplitude * 0.12
    ring.transform = CGAffineTransform(scaleX: ringScale, y: ringScale)
    if presentation != .standard {
      label.font = UIFontMetrics(forTextStyle: .footnote).scaledFont(
        for: .systemFont(ofSize: 13, weight: .medium), maximumPointSize: 18)
      label.adjustsFontForContentSizeCategory = false
      label.adjustsFontSizeToFitWidth = true
      label.minimumScaleFactor = 0.6
      waves.transform = presentation == .helpWrite ? CGAffineTransform(scaleX: 0.55, y: 0.55) : .identity
    }
    let preparationColor = !phase.isBusy && presentation != .video ? unavailableTitleColor : nil
    // Both idle actions keep the same tint, surface, label and optical center.
    // Recording is the only red state; idle/processing keep the neutral brand surface.
    let surface: UIColor = recording ? .systemRed : (preparationColor == nil ? .label : .secondarySystemBackground)
    if !(changed || presentationChanged) { shape.backgroundColor = surface }
    ring.backgroundColor = recording ? UIColor.systemRed.withAlphaComponent(0.16) : UIColor.label.withAlphaComponent(0.08)
    image.tintColor = preparationColor ?? .systemBackground
    label.textColor = .systemBackground
    idleTitle.textColor = preparationColor ?? .systemBackground
    bars.forEach { $0.backgroundColor = .white }
    shape.alpha = isTracking ? (pressInside ? 0.6 : 0.35) : (available || phase.isBusy || onUnavailable != nil ? 1 : 0.55)
    accessibilityLabel = localizer.text(recording ? "结束录音" : busy ? (label.text ?? "thinking") : idleLabel)
    accessibilityTraits = available || phase == .recording || phase == .starting || (!phase.isBusy && onUnavailable != nil)
      ? .button : [.button, .notEnabled]
    accessibilityHint = localizer.text(!available && !phase.isBusy && onUnavailable != nil
      ? "查看或完成语音准备，不会开始录音。"
      : "点按开始，再点结束；按住说话，松手结束；移出按钮取消。")
    if changed || presentationChanged {
      animator?.stopAnimation(false)
      animator?.finishAnimation(at: .current)
      let small = compact || presentation == .helpWrite
      width.constant = small ? 52 : recording ? (presentation == .video ? 96 : 112) : busy ? 129 : 147
      height.constant = small ? 52 : recording ? (presentation == .video ? 96 : 112) : 60
      if compact { width.constant = 38; height.constant = 38 }
      let animations = {
        self.shape.backgroundColor = surface
        self.shape.layer.cornerRadius = self.height.constant / 2
        self.ring.layer.cornerRadius = (self.height.constant + 24) / 2
        self.layoutIfNeeded()
      }
      if UIAccessibility.isReduceMotionEnabled || !changed {
        animations()
      } else {
        let next = UIViewPropertyAnimator(duration: 0.2, curve: .easeOut, animations: animations)
        animator = next
        next.startAnimation()
      }
    }
    if recording {
      levels.removeFirst()
      levels.append(amplitude)
      let profile: [CGFloat] = [0.45, 0.7, 0.9, 1, 0.9, 0.7, 0.45]
      for (index, constraint) in barHeights.enumerated() {
        constraint.constant = 5 + levels[index] * 44 * profile[index]
      }
      if UIAccessibility.isReduceMotionEnabled {
        waves.layoutIfNeeded()
      } else {
        UIView.animate(
          withDuration: 0.08, delay: 0, options: [.beginFromCurrentState, .allowUserInteraction]
        ) { self.waves.layoutIfNeeded() }
      }
    }
    updateProcessingAnimation()
  }

  override func layoutSubviews() {
    super.layoutSubviews()
    CATransaction.begin()
    CATransaction.setDisableActions(true)
    processingSweep.frame = shape.bounds
    processingSweep.colors = [UIColor.clear.cgColor,
      UIColor.systemBackground.withAlphaComponent(0.24).resolvedColor(with: traitCollection).cgColor,
      UIColor.clear.cgColor]
    CATransaction.commit()
    if presentation == .helpWrite {
      accessibilityPath = UIBezierPath(ovalIn: UIAccessibility.convertToScreenCoordinates(shape.frame, in: self))
    } else {
      accessibilityPath = nil
    }
    updateProcessingAnimation()
  }
  override func point(inside point: CGPoint, with event: UIEvent?) -> Bool {
    presentation == .helpWrite ? containsHelpWriteTouch(point) : super.point(inside: point, with: event)
  }
  private func containsHelpWriteTouch(_ point: CGPoint) -> Bool {
    let radius = max(44, min(width.constant, height.constant)) / 2 + 8
    let dx = point.x - bounds.midX
    let dy = point.y - bounds.midY
    return dx * dx + dy * dy <= radius * radius
  }
  private func containsTrackedTouch(_ point: CGPoint) -> Bool {
    presentation == .helpWrite ? containsHelpWriteTouch(point) : bounds.insetBy(dx: -10, dy: -10).contains(point)
  }
  @objc private func reduceMotionChanged() {
    if UIAccessibility.isReduceMotionEnabled {
      animator?.stopAnimation(false)
      animator?.finishAnimation(at: .end)
      animator = nil
      ring.transform = .identity
    }
    updateProcessingAnimation()
  }
  @objc private func windowVisibilityChanged(_ notification: Notification) {
    guard let changed = notification.object as? UIWindow, changed === window else { return }
    updateProcessingAnimation()
  }
  @objc private func applicationWillResignActive() {
    applicationIsActive = false
    updateProcessingAnimation()
  }
  @objc private func applicationDidBecomeActive() {
    applicationIsActive = true
    updateProcessingAnimation()
  }
  private func updateProcessingAnimation() {
    // Never animate an idle, hidden, offscreen, detached or reduced-motion view.
    var visible = presentationIsVisible && applicationIsActive && window != nil
    var ancestor: UIView? = self
    while let current = ancestor {
      visible = visible && !current.isHidden && current.alpha > 0
      ancestor = current.superview
    }
    let animate = visible && presentation != .standard && phase == .processing
      && !UIAccessibility.isReduceMotionEnabled
    processingSweep.isHidden = !animate
    guard animate else {
      processingSweep.removeAnimation(forKey: "openless.processingSweep")
      return
    }
    guard shape.bounds.width > 0,
      processingSweep.animation(forKey: "openless.processingSweep") == nil else { return }
    let animation = CABasicAnimation(keyPath: "transform.translation.x")
    animation.fromValue = -shape.bounds.width
    animation.toValue = shape.bounds.width
    animation.duration = 1.35
    animation.repeatCount = .infinity
    processingSweep.add(animation, forKey: "openless.processingSweep")
  }
  override var isHidden: Bool {
    didSet { updateProcessingAnimation() }
  }
  override func traitCollectionDidChange(_ previousTraitCollection: UITraitCollection?) {
    super.traitCollectionDidChange(previousTraitCollection)
    setNeedsLayout()
  }
  override func beginTracking(_ touch: UITouch, with event: UIEvent?) -> Bool {
    unavailablePress = !available && !phase.isBusy && onUnavailable != nil
    guard available || phase == .recording || phase == .starting || unavailablePress else { return false }
    pressInside = true
    let now = ProcessInfo.processInfo.systemUptime
    pressOrigin = touch.location(in: self)
    pressLocation = pressOrigin
    translationGesture.down(at: now, allowed: !phase.isBusy && onTranslationTouch != nil)
    gesture.down(at: now, phase: phase)
    shape.alpha = 0.6
    holdTimer?.invalidate()
    holdTimer = Timer.scheduledTimer(withTimeInterval: VoiceGesture.holdDelay, repeats: false) {
      [weak self] _ in
      MainActor.assumeIsolated {
        guard let self else { return }
        if self.beginTranslationIfNeeded() { return }
        guard !self.unavailablePress else { return }
        guard self.pressInside else {
          _ = self.gesture.cancel()
          return
        }
        if let action = self.gesture.tick(at: ProcessInfo.processInfo.systemUptime) {
          self.onAction?(action)
        }
      }
    }
    if let holdTimer { RunLoop.main.add(holdTimer, forMode: .common) }
    return true
  }
  override func continueTracking(_ touch: UITouch, with event: UIEvent?) -> Bool {
    pressLocation = touch.location(in: self)
    if isSelectingTranslation || beginTranslationIfNeeded() {
      _ = onTranslationTouch?(.moved, pressLocation)
      return true
    }
    pressInside = containsTrackedTouch(pressLocation)
    shape.alpha = pressInside ? 0.6 : 0.35
    return true
  }
  private func beginTranslationIfNeeded() -> Bool {
    guard translationGesture.move(at: ProcessInfo.processInfo.systemUptime,
      horizontal: pressLocation.x - pressOrigin.x, vertical: pressLocation.y - pressOrigin.y) else { return false }
    guard onTranslationTouch?(.began, pressLocation) == true else {
      translationGesture.end()
      return false
    }
    holdTimer?.invalidate(); holdTimer = nil
    unavailablePress = false
    // The owner cancelled the provisional dictation before showing the arc.
    // Retire the old press so its later release cannot stop the new recording.
    _ = gesture.cancel()
    return true
  }
  override func endTracking(_ touch: UITouch?, with event: UIEvent?) {
    holdTimer?.invalidate()
    holdTimer = nil
    if let touch {
      pressLocation = touch.location(in: self)
      if !isSelectingTranslation { _ = beginTranslationIfNeeded() }
    }
    let inside =
      touch.map { containsTrackedTouch($0.location(in: self)) } ?? false
    if translationGesture.end() {
      unavailablePress = false
      _ = onTranslationTouch?(touch == nil ? .cancelled : .ended, touch?.location(in: self) ?? pressLocation)
    } else if unavailablePress {
      unavailablePress = false
      if inside { onUnavailable?() }
    } else if let action = gesture.up(phase: phase, inside: inside) { onAction?(action) }
    shape.alpha = available || phase.isBusy || onUnavailable != nil ? 1 : 0.55
  }
  override func cancelTracking(with event: UIEvent?) {
    holdTimer?.invalidate()
    holdTimer = nil
    unavailablePress = false
    if translationGesture.end() { _ = onTranslationTouch?(.cancelled, pressLocation) }
    else if let action = gesture.cancel() { onAction?(action) }
    shape.alpha = available || phase.isBusy || onUnavailable != nil ? 1 : 0.55
  }
  override func accessibilityActivate() -> Bool {
    if !available && !phase.isBusy, let onUnavailable { onUnavailable(); return true }
    guard available || phase == .recording || phase == .starting else { return false }
    onAction?(phase == .recording ? .stop : phase == .starting ? .cancel : .start)
    return true
  }
  override func didMoveToWindow() {
    super.didMoveToWindow()
    updateProcessingAnimation()
    if window == nil {
      unavailablePress = false
      holdTimer?.invalidate()
      holdTimer = nil
      if translationGesture.end() { _ = onTranslationTouch?(.cancelled, pressLocation) }
      else if let action = gesture.cancel() { onAction?(action) }
      animator?.stopAnimation(true)
      animator = nil
    }
  }
}
