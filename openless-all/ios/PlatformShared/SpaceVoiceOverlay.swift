import OpenLessShared
import UIKit

/// A single recording surface keeps preparation, capture and processing distinct.
final class SpaceVoiceOverlay: UIView {
  var onAction: ((SpaceVoiceGesture.Action) -> Void)?
  private let glow = CAGradientLayer()
  private let caption = UILabel()
  private let instruction = UILabel()
  private let waveform = UIStackView()
  private let spinner = UIActivityIndicatorView(style: .medium)
  private let cancelProcessing = UIButton(type: .system)
  private var heights: [NSLayoutConstraint] = []
  private var buttons: [(SpaceVoiceGesture.Action, UIButton)] = []
  private(set) var gesture = SpaceVoiceGesture()
  private(set) var presentation: KeyboardVoicePresentation = .idle
  private var canTranslate = false
  private var canSend = false
  private var fixedOutput = false
  private var outputTitle: String?
  private var compactLayout: Bool?
  private var locale = InterfaceLocalizer(language: .system)

  override init(frame: CGRect) {
    super.init(frame: frame)
    backgroundColor = .clear
    isOpaque = false
    layer.addSublayer(glow)
    glow.type = .radial
    glow.startPoint = CGPoint(x: 0.5, y: 0.35)
    glow.endPoint = CGPoint(x: 1, y: 0.9)
    glow.locations = [0, 0.48, 1]
    caption.textAlignment = .center
    caption.font = .systemFont(ofSize: 15, weight: .medium)
    caption.textColor = .secondaryLabel
    caption.accessibilityIdentifier = "spaceVoice.status"
    addSubview(caption)
    instruction.font = .systemFont(ofSize: 13, weight: .medium)
    instruction.textAlignment = .center
    instruction.textColor = .secondaryLabel
    instruction.adjustsFontSizeToFitWidth = true
    instruction.minimumScaleFactor = 0.8
    instruction.accessibilityIdentifier = "spaceVoice.instruction"
    addSubview(instruction)
    waveform.axis = .horizontal
    waveform.alignment = .center
    waveform.spacing = 4
    waveform.isAccessibilityElement = false
    addSubview(waveform)
    for index in 0..<19 {
      let bar = UIView()
      bar.backgroundColor = index < 10 ? .systemTeal : .systemBlue
      bar.layer.cornerRadius = 2
      let height = bar.heightAnchor.constraint(equalToConstant: 3)
      NSLayoutConstraint.activate([bar.widthAnchor.constraint(equalToConstant: 3), height])
      heights.append(height)
      waveform.addArrangedSubview(bar)
    }
    spinner.color = .secondaryLabel
    spinner.hidesWhenStopped = true
    addSubview(spinner)
    var cancel = UIButton.Configuration.plain()
    cancel.image = UIImage(systemName: "xmark")
    cancel.imagePadding = 6
    cancel.baseForegroundColor = .secondaryLabel
    cancelProcessing.configuration = cancel
    cancelProcessing.accessibilityIdentifier = "spaceVoice.cancelProcessing"
    cancelProcessing.addAction(UIAction { [weak self] _ in self?.onAction?(.cancel) }, for: .touchUpInside)
    addSubview(cancelProcessing)
    for (action, symbol) in [(SpaceVoiceGesture.Action.cancel, "xmark"),
      (.translate, "character.bubble"), (.send, "arrow.up")] {
      let button = UIButton(type: .system)
      var config = UIButton.Configuration.filled()
      config.image = UIImage(systemName: symbol)
      config.imagePlacement = .top
      config.titleLineBreakMode = .byTruncatingTail
      config.subtitleLineBreakMode = .byTruncatingTail
      config.contentInsets = .init(top: 4, leading: 4, bottom: 4, trailing: 4)
      config.cornerStyle = .capsule
      config.titleTextAttributesTransformer = UIConfigurationTextAttributesTransformer { value in
        var value = value
        value.font = .systemFont(ofSize: 14, weight: .medium)
        return value
      }
      config.subtitleTextAttributesTransformer = UIConfigurationTextAttributesTransformer { value in
        var value = value
        value.font = .systemFont(ofSize: 11, weight: .regular)
        return value
      }
      button.configuration = config
      button.accessibilityIdentifier = "spaceVoice." + String(describing: action)
      button.addAction(UIAction { [weak self] _ in self?.onAction?(action) }, for: .touchUpInside)
      buttons.append((action, button))
      addSubview(button)
    }
    isHidden = true
    accessibilityIdentifier = "spaceVoiceOverlay"
    accessibilityViewIsModal = true
  }
  required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

  override func layoutSubviews() {
    super.layoutSubviews()
    glow.frame = bounds
    glow.colors = [UIColor.systemTeal.withAlphaComponent(0.12).cgColor,
      UIColor.systemBlue.withAlphaComponent(0.04).cgColor, UIColor.clear.cgColor]
    let compact = bounds.height < 180
    if compactLayout != compact {
      compactLayout = compact
      for (_, button) in buttons {
        button.configuration?.preferredSymbolConfigurationForImage = .init(pointSize: compact ? 14 : 19)
        button.configuration?.imagePadding = compact ? 2 : 5
      }
    }
    let width = max(0, (bounds.width - 32) / 3)
    let y = compact ? 49 : max(70, bounds.height * 0.46)
    let buttonHeight: CGFloat = compact ? 52 : 68
    for (index, pair) in buttons.enumerated() {
      let button = pair.1
      button.transform = .identity
      button.frame = CGRect(x: 8 + CGFloat(index) * (width + 8),
        y: y - (index == 1 && !compact ? 8 : 0), width: width, height: buttonHeight)
      button.transform = compact ? .identity : CGAffineTransform(rotationAngle: CGFloat(index - 1) * 0.07)
    }
    waveform.frame = CGRect(x: (bounds.width - 129) / 2, y: compact ? 0 : max(16, bounds.height * 0.17),
      width: 129, height: compact ? 24 : 32)
    let processing = presentation == .processing
    caption.frame = CGRect(x: 16, y: processing ? bounds.height * 0.42 : waveform.frame.maxY + (compact ? 2 : 6),
      width: bounds.width - 32, height: compact ? 18 : 22)
    instruction.frame = CGRect(x: 12, y: min(bounds.height - 25, y + buttonHeight + 16),
      width: bounds.width - 24, height: 21)
    spinner.center = CGPoint(x: bounds.midX, y: processing ? caption.frame.minY - 24 : waveform.center.y)
    cancelProcessing.frame = CGRect(x: bounds.midX - 60, y: min(bounds.height - 48, caption.frame.maxY + 24),
      width: 120, height: 44)
  }

  func begin(canTranslate: Bool, canSend: Bool, language: InterfaceLanguage,
    fixedOutput: Bool = false, outputTitle: String? = nil) {
    self.canTranslate = canTranslate
    self.canSend = canSend
    self.fixedOutput = fixedOutput
    self.outputTitle = outputTitle
    locale = InterfaceLocalizer(language: language)
    buttons[0].1.configuration?.title = locale.text("撤回输入")
    buttons[1].1.configuration?.title = locale.text("翻译")
    buttons[1].1.configuration?.subtitle = fixedOutput ? (outputTitle ?? locale.text("未选择语言")) : nil
    buttons[2].1.configuration?.title = locale.text(canSend ? "发送" : "完成输入")
    buttons[1].1.alpha = canTranslate ? 1 : 0.45
    cancelProcessing.configuration?.title = locale.text("取消")
    gesture.begin()
    update(presentation: .preparing, level: 0)
    updateSelection()
    layoutIfNeeded()
  }

  func update(presentation: KeyboardVoicePresentation, level: Float) {
    guard presentation != .idle else { dismiss(); return }
    self.presentation = presentation
    isHidden = false
    accessibilityValue = String(describing: presentation)
    let processing = presentation == .processing
    let recording = presentation == .recording
    if processing { gesture.cancel() }
    caption.text = locale.text(presentation.title)
    waveform.isHidden = !recording
    instruction.isHidden = processing
    buttons.forEach { $0.1.isHidden = processing }
    cancelProcessing.isHidden = !processing
    glow.isHidden = !recording
    if recording { spinner.stopAnimating() } else { spinner.startAnimating() }
    let amplitude = CGFloat(VoiceWaveform.amplitude(rms: recording ? level : 0))
    for (index, height) in heights.enumerated() {
      let profile = 1 - CGFloat(abs(index - 9)) / 12
      height.constant = 3 + (bounds.height < 180 ? 14 : 26) * amplitude * profile
    }
    setNeedsLayout()
    if UIAccessibility.isReduceMotionEnabled { waveform.layoutIfNeeded() }
    else {
      UIView.animate(withDuration: 0.09, delay: 0, options: [.beginFromCurrentState, .allowUserInteraction]) {
        self.waveform.layoutIfNeeded()
      }
    }
  }

  func move(to point: CGPoint) -> Bool {
    let old = gesture.selection
    gesture.move(hit: hit(at: point), inRestArea: restArea(point))
    if gesture.active, old != gesture.selection { updateSelection() }
    return old != gesture.selection
  }
  func release(at point: CGPoint) -> SpaceVoiceGesture.Action? {
    gesture.release(hit: hit(at: point), inRestArea: restArea(point))
  }
  func dismiss() {
    gesture.cancel()
    spinner.stopAnimating()
    presentation = .idle
    accessibilityValue = "idle"
    isHidden = true
  }
  private func hit(at point: CGPoint) -> SpaceVoiceGesture.Action? {
    guard bounds.contains(point) else { return nil }
    return buttons.first { $0.1.bounds.insetBy(dx: -3, dy: -3).contains($0.1.convert(point, from: self)) }?.0
  }
  private func restArea(_ point: CGPoint) -> Bool {
    bounds.contains(point) && point.y >= (buttons.map { $0.1.frame.maxY }.max() ?? bounds.midY) + 4
  }
  private func updateSelection() {
    for (action, button) in buttons {
      let selected = action == gesture.selection
      button.configuration?.baseBackgroundColor = selected ? .white : .tertiarySystemFill
      button.configuration?.baseForegroundColor = selected ? (action == .cancel ? .systemRed : .black) : .label
      button.isSelected = selected
    }
    let title: String = switch gesture.selection {
    case .input: "松手输入"
    case .cancel: "松手撤回本次输入"
    case .translate: canTranslate ? (fixedOutput ? "松手翻译" : "松手选择翻译语言") : "请先配置翻译模型和语言"
    case .send: canSend ? "松手发送" : "松手完成输入"
    }
    instruction.text = locale.text(title)
    if fixedOutput, canTranslate, gesture.selection == .translate, let outputTitle {
      instruction.text = locale.text(title) + " · " + outputTitle
    }
  }
}
