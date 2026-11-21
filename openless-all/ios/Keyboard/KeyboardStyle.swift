import KeyboardKit
import SwiftUI
import OpenLessShared
import UIKit

/// UIKit's input view owns the continuous system keyboard backdrop on iOS 16+.
/// Keep content transparent; individual keys retain their own surfaces.
enum KeyboardStyle {
  static let canvas = UIColor.clear
  // Keep all modes the same height and give four key rows their full size.
  static let voicePanelHeight = CGFloat(KeyboardSizing.defaultBodyHeight)
  // Keyboard chrome has a bounded height. Scale hints without covering edit keys;
  // full generated text remains available in the separate scrolling preview.
  static var detailFont: UIFont {
    UIFontMetrics(forTextStyle: .footnote).scaledFont(
      for: .systemFont(ofSize: 13), maximumPointSize: 16)
  }
  static let key = UIColor { traits in
    traits.userInterfaceStyle == .dark
      ? UIColor(red: 0.255, green: 0.27, blue: 0.30, alpha: 1) : .white
  }
  static let auxiliary = UIColor { traits in
    traits.userInterfaceStyle == .dark
      ? UIColor(red: 0.185, green: 0.20, blue: 0.23, alpha: 1)
      : UIColor(red: 0.82, green: 0.85, blue: 0.89, alpha: 1)
  }
  static let accent = UIColor { traits in
    traits.userInterfaceStyle == .dark
      ? UIColor(red: 0.44, green: 0.62, blue: 1, alpha: 1)
      : UIColor(red: 0.17, green: 0.37, blue: 0.93, alpha: 1)
  }
}

/// A single, interruptible selection surface moves between explicit modes.
final class KeyboardModeControl: UIView {
  var onSelect: ((Int) -> Void)?
  private let selection = UIView()
  private var buttons: [UIButton] = []
  private var selectedIndex = 0
  private var animator: UIViewPropertyAnimator?
  override init(frame: CGRect) {
    super.init(frame: frame)
    backgroundColor = .clear
    layer.cornerRadius = 22
    // A lifted white "thumb" reads clearly against the gray track in both appearances.
    selection.backgroundColor = KeyboardStyle.auxiliary
    selection.layer.cornerRadius = 19
    selection.layer.shadowColor = UIColor.black.cgColor
    selection.layer.shadowOpacity = 0
    selection.layer.shadowRadius = 2
    selection.layer.shadowOffset = CGSize(width: 0, height: 1)
    selection.isUserInteractionEnabled = false
    addSubview(selection)
    let stack = UIStackView()
    stack.axis = .horizontal
    stack.distribution = .fillEqually
    stack.translatesAutoresizingMaskIntoConstraints = false
    addSubview(stack)
    NSLayoutConstraint.activate([
      stack.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 3),
      stack.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -3),
      stack.topAnchor.constraint(equalTo: topAnchor),
      stack.bottomAnchor.constraint(equalTo: bottomAnchor),
    ])
    for (index, title) in ["语音", "EN", "拼"].enumerated() {
      let button = UIButton(type: .custom)
      button.setTitle(index == 0 ? nil : title, for: .normal)
      if index == 0 {
        button.setImage(UIImage(systemName: "waveform", withConfiguration:
          UIImage.SymbolConfiguration(pointSize: 23, weight: .medium)), for: .normal)
        button.accessibilityLabel = "语音"
      }
      button.titleLabel?.font = .systemFont(ofSize: 17, weight: .semibold)
      button.accessibilityIdentifier = ["keyboardModeVoice", "keyboardModeEnglish", "keyboardModePinyin"][index]
      button.addAction(UIAction { [weak self] _ in self?.onSelect?(index) }, for: .touchUpInside)
      stack.addArrangedSubview(button)
      buttons.append(button)
    }
    select(0, animated: false)
  }
  required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }
  override func layoutSubviews() {
    super.layoutSubviews()
    let width = max(0, bounds.width - 6) / 3
    selection.bounds = CGRect(x: 0, y: 0, width: width, height: max(0, bounds.height - 6))
    selection.center = CGPoint(x: 3 + width / 2, y: bounds.midY)
    selection.transform = CGAffineTransform(translationX: CGFloat(selectedIndex) * width, y: 0)
  }
  func setInterfaceLanguage(_ language: InterfaceLanguage) {
    let localizer = InterfaceLocalizer(language: language)
    for (index, key) in ["语音", "EN", "拼"].enumerated() {
      buttons[index].setTitle(index == 0 ? nil : localizer.text(key), for: .normal)
      buttons[index].accessibilityLabel = localizer.text(key)
      buttons[index].titleLabel?.adjustsFontSizeToFitWidth = true
      buttons[index].titleLabel?.minimumScaleFactor = 0.8
    }
  }
  func select(_ index: Int, animated: Bool) {
    guard buttons.indices.contains(index) else { return }
    let changed = index != selectedIndex
    selectedIndex = index
    for (position, button) in buttons.enumerated() {
      let selected = position == index
      button.setTitleColor(selected ? .label : .secondaryLabel, for: .normal)
      button.tintColor = selected ? .label : .secondaryLabel
      button.titleLabel?.font = .systemFont(ofSize: 17, weight: selected ? .bold : .semibold)
      button.isSelected = selected
      button.accessibilityTraits = selected ? [.button, .selected] : .button
    }
    animator?.stopAnimation(false)
    animator?.finishAnimation(at: .current)
    let move = {
      self.selection.transform = CGAffineTransform(
        translationX: CGFloat(index) * max(0, self.bounds.width - 6) / 3, y: 0)
    }
    guard changed, animated, window != nil, !UIAccessibility.isReduceMotionEnabled else {
      move(); return
    }
    let next = UIViewPropertyAnimator(duration: 0.2, dampingRatio: 1, animations: move)
    animator = next
    next.startAnimation()
  }
  override func didMoveToWindow() {
    super.didMoveToWindow()
    if window == nil { animator?.stopAnimation(true); animator = nil }
  }
}
