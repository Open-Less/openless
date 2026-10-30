import OpenLessShared
import UIKit

/// Three visible targets form an arc; paging keeps every advertised target reachable.
/// All geometry lives in this view so a release is tested against the visible page.
final class TranslationArcView: UIView {
  var onChoose: ((String) -> Void)?
  var onCancel: (() -> Void)?
  var interfaceLanguage: InterfaceLanguage = .system {
    didSet {
      guard interfaceLanguage != oldValue else { return }
      previous.setTitle(uiText.text("‹ 上一组"), for: .normal)
      nextPage.setTitle(uiText.text("下一组 ›"), for: .normal)
      cancel.accessibilityLabel = uiText.text("取消翻译选择")
      updateSelection(); setNeedsLayout()
    }
  }
  private var uiText: InterfaceLocalizer { InterfaceLocalizer(language: interfaceLanguage) }
  private let scroll = UIScrollView()
  private let content = UIView()
  private let hint = UILabel()
  private let cancellationSurface = TranslationCancellationSurface()
  private let pages = UIStackView()
  private let previous = UIButton(type: .system)
  private let nextPage = UIButton(type: .system)
  private let cancel = UIButton(type: .system)
  private var buttons: [UIButton] = []
  private var targets: [KeyboardVoiceOption] = []
  private var page = 0
  private var edgeHeld = false
  private(set) var gesture = TranslationSelectionGesture()

  override init(frame: CGRect) {
    super.init(frame: frame)
    backgroundColor = KeyboardStyle.canvas
    accessibilityViewIsModal = true
    scroll.contentInsetAdjustmentBehavior = .never
    scroll.showsVerticalScrollIndicator = false
    addSubview(scroll)
    scroll.addSubview(content)
    cancellationSurface.isUserInteractionEnabled = false
    content.addSubview(cancellationSurface)
    hint.font = .preferredFont(forTextStyle: .footnote)
    hint.adjustsFontForContentSizeCategory = true
    hint.textAlignment = .center
    hint.numberOfLines = 0
    content.addSubview(hint)
    pages.axis = .horizontal
    pages.distribution = .equalSpacing
    previous.setTitle(uiText.text("‹ 上一组"), for: .normal)
    nextPage.setTitle(uiText.text("下一组 ›"), for: .normal)
    for button in [previous, nextPage] {
      button.titleLabel?.font = .preferredFont(forTextStyle: .footnote)
      button.titleLabel?.adjustsFontForContentSizeCategory = true
    }
    previous.accessibilityIdentifier = "translationPreviousPage"
    nextPage.accessibilityIdentifier = "translationNextPage"
    previous.addAction(UIAction { [weak self] _ in self?.turn(-1) }, for: .touchUpInside)
    nextPage.addAction(UIAction { [weak self] _ in self?.turn(1) }, for: .touchUpInside)
    pages.addArrangedSubview(previous)
    pages.addArrangedSubview(nextPage)
    content.addSubview(pages)
    cancel.titleLabel?.font = .preferredFont(forTextStyle: .body)
    cancel.titleLabel?.adjustsFontForContentSizeCategory = true
    cancel.titleLabel?.numberOfLines = 0
    cancel.titleLabel?.textAlignment = .center
    cancel.tintColor = .label
    cancel.contentVerticalAlignment = .top
    cancel.accessibilityLabel = uiText.text("取消翻译选择")
    cancel.accessibilityIdentifier = "translationCancel"
    cancel.addAction(UIAction { [weak self] _ in self?.onCancel?() }, for: .touchUpInside)
    content.addSubview(cancel)
  }
  required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

  override func layoutSubviews() {
    super.layoutSubviews()
    scroll.frame = bounds
    let width = bounds.width
    guard width > 0 else { return }
    let vertical = traitCollection.preferredContentSizeCategory.isAccessibilityCategory
    let compact = !vertical && bounds.height <= 240
    let hintHeight = max(20, hint.sizeThatFits(CGSize(width: max(1, width - 32), height: .greatestFiniteMagnitude)).height)
    hint.frame = CGRect(x: 16, y: compact ? 6 : 20, width: max(1, width - 32), height: hintHeight)
    let count = max(1, buttons.count)
    let gap: CGFloat = 10
    let buttonWidth = vertical ? max(44, width - 32)
      : min(200, max(44, (width - 32 - CGFloat(count - 1) * gap) / CGFloat(count)))
    let buttonHeight = max(52, buttons.map {
      ($0.titleLabel?.sizeThatFits(CGSize(width: buttonWidth - 16, height: .greatestFiniteMagnitude)).height ?? 20) + 20
    }.max() ?? 52)
    let rowWidth = CGFloat(count) * buttonWidth + CGFloat(count - 1) * gap
    let rowY = hint.frame.maxY + (compact ? 6 : 22)
    for (index, button) in buttons.enumerated() {
      let offset = CGFloat(index) - CGFloat(buttons.count - 1) / 2
      button.transform = .identity
      button.bounds = CGRect(x: 0, y: 0, width: buttonWidth, height: buttonHeight)
      button.center = vertical
        ? CGPoint(x: width / 2, y: rowY + buttonHeight / 2 + CGFloat(index) * (buttonHeight + gap))
        : CGPoint(x: (width - rowWidth) / 2 + buttonWidth / 2 + CGFloat(index) * (buttonWidth + gap),
          y: rowY + buttonHeight / 2 + abs(offset) * 12)
      button.layer.cornerRadius = min(buttonWidth, buttonHeight) / 2
      button.transform = vertical ? .identity : CGAffineTransform(rotationAngle: offset * 0.13)
    }
    // A broad curved cancellation region mirrors the reference and gives users
    // an unambiguous place to release outside every target. Large type can scroll.
    // Include the lower edge of rotated outer targets, not just the unrotated row.
    let targetsBottom = buttons.map { $0.frame.maxY }.max() ?? rowY
    let cancelY = targetsBottom + (compact ? 6 : 16)
    let pageTitleHeight = max(previous.titleLabel?.intrinsicContentSize.height ?? 20,
      nextPage.titleLabel?.intrinsicContentSize.height ?? 20)
    let pagesHeight: CGFloat = pages.isHidden ? 0 : max(52, pageTitleHeight + 16)
    cancellationSurface.isHidden = vertical
    let cancelTitleHeight = cancel.titleLabel?.sizeThatFits(CGSize(width: max(1, width - 32), height: .greatestFiniteMagnitude)).height ?? 24
    let cancelInset: CGFloat = compact ? 4 : 14
    let bottomInset: CGFloat = compact ? 6 : 0
    let minimumCancelHeight = compact ? max(44, cancelTitleHeight + 8) : max(66, cancelTitleHeight + 26)
    let contentHeight = max(bounds.height, cancelY + cancelInset + minimumCancelHeight + bottomInset + pagesHeight)
    content.frame = CGRect(x: 0, y: 0, width: width, height: contentHeight)
    cancellationSurface.frame = CGRect(x: 0, y: cancelY - 4, width: width, height: contentHeight - cancelY + 4)
    cancel.frame = CGRect(x: 16, y: cancelY + cancelInset, width: max(1, width - 32),
      height: max(44, contentHeight - cancelY - cancelInset - bottomInset - pagesHeight))
    pages.frame = CGRect(x: 16, y: contentHeight - pagesHeight, width: max(1, width - 32), height: pagesHeight)
    scroll.contentSize = content.bounds.size
  }

  func begin(targets: [KeyboardVoiceOption]) {
    self.targets = targets
    page = 0
    edgeHeld = false
    gesture.begin(targets: targets)
    scroll.setContentOffset(.zero, animated: false)
    rebuild()
  }
  private func turn(_ delta: Int) {
    let next = min(max(0, (targets.count - 1) / 3), max(0, page + delta))
    guard next != page else { return }
    page = next
    gesture.move(hitID: nil)
    rebuild()
  }
  private func rebuild() {
    buttons.forEach { $0.removeFromSuperview() }
    buttons = []
    for option in targets.dropFirst(page * 3).prefix(3) {
      let button = UIButton(type: .system)
      button.setTitle(option.title, for: .normal)
      button.titleLabel?.font = .preferredFont(forTextStyle: .subheadline)
      button.titleLabel?.adjustsFontForContentSizeCategory = true
      button.titleLabel?.numberOfLines = 3
      button.titleLabel?.textAlignment = .center
      button.accessibilityIdentifier = "translationTarget.\(option.id)"
      button.accessibilityHint = uiText.text("选择此语言并开始翻译录音")
      button.addAction(UIAction { [weak self] _ in self?.onChoose?(option.id) }, for: .touchUpInside)
      content.addSubview(button)
      buttons.append(button)
    }
    previous.isEnabled = page > 0
    nextPage.isEnabled = (page + 1) * 3 < targets.count
    pages.isHidden = targets.count <= 3
    updateSelection()
    setNeedsLayout()
    layoutIfNeeded()
  }
  func move(to point: CGPoint) {
    guard gesture.isSelecting else { return }
    let edge = bounds.contains(point) && point.y < bounds.midY && (point.x < 12 || point.x > bounds.width - 12)
    if edge && !edgeHeld { turn(point.x < 12 ? -1 : 1) }
    edgeHeld = edge
    gesture.move(hitID: target(at: point))
    updateSelection()
  }
  /// The terminal touch is authoritative, even if UIKit coalesced the last move.
  /// Releasing at a paging edge never changes pages and selects an unseen target.
  func release(at point: CGPoint) -> String? {
    guard !isHidden else { interrupt(); return nil }
    layoutIfNeeded()
    gesture.move(hitID: target(at: point))
    let selected = gesture.release()
    edgeHeld = false
    updateSelection()
    return selected
  }
  func interrupt() {
    gesture.cancel()
    edgeHeld = false
    updateSelection()
  }
  private func target(at point: CGPoint) -> String? {
    guard bounds.contains(point), gesture.isSelecting else { return nil }
    // Convert into each rotated button, not its enclosing axis-aligned rectangle.
    return buttons.enumerated().first { _, button in
      button.point(inside: button.convert(point, from: self), with: nil)
    }.map { targets[page * 3 + $0.offset].id }
  }
  private func updateSelection() {
    let accessible = traitCollection.preferredContentSizeCategory.isAccessibilityCategory
    hint.text = uiText.text(gesture.selectedID == nil ? (accessible ? "选择翻译语言" : "向上滑动以选择") : "释放以翻译")
    cancel.setTitle(uiText.text(gesture.selectedID == nil && !accessible ? "松开以取消" : "取消"), for: .normal)
    for (index, button) in buttons.enumerated() {
      let selected = targets[page * 3 + index].id == gesture.selectedID
      button.backgroundColor = selected ? .label : .label.withAlphaComponent(0.075)
      button.setTitleColor(selected ? .systemBackground : .label, for: .normal)
      button.accessibilityTraits = selected ? [.button, .selected] : .button
    }
  }
  override func traitCollectionDidChange(_ previousTraitCollection: UITraitCollection?) {
    super.traitCollectionDidChange(previousTraitCollection)
    updateSelection()
    setNeedsLayout()
  }
  override func didMoveToWindow() {
    super.didMoveToWindow()
    if window == nil { interrupt() }
  }
}

/// Static, inexpensive shading; it never implies recording or processing activity.
private final class TranslationCancellationSurface: UIView {
  private let gradient = CAGradientLayer()
  private let outline = CAShapeLayer()
  override init(frame: CGRect) {
    super.init(frame: frame)
    gradient.mask = outline
    layer.addSublayer(gradient)
  }
  required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }
  override func layoutSubviews() {
    super.layoutSubviews()
    CATransaction.begin()
    CATransaction.setDisableActions(true)
    gradient.frame = bounds
    gradient.colors = [UIColor.label.withAlphaComponent(0.19).resolvedColor(with: traitCollection).cgColor,
      UIColor.label.withAlphaComponent(0.025).resolvedColor(with: traitCollection).cgColor]
    let path = UIBezierPath()
    path.move(to: CGPoint(x: 0, y: 30))
    path.addQuadCurve(to: CGPoint(x: bounds.width, y: 30), controlPoint: CGPoint(x: bounds.midX, y: -30))
    path.addLine(to: CGPoint(x: bounds.width, y: bounds.height))
    path.addLine(to: CGPoint(x: 0, y: bounds.height))
    path.close()
    outline.path = path.cgPath
    CATransaction.commit()
  }
  override func traitCollectionDidChange(_ previousTraitCollection: UITraitCollection?) {
    super.traitCollectionDidChange(previousTraitCollection)
    setNeedsLayout()
  }
}
