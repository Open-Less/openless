import SwiftUI
import UIKit
import CoreImage

/// UIKit owns geometry and items together; SwiftUI only supplies selection.
struct ProductDock: View {
  @Binding var tab: Int
  @Binding var section: StyleLibrarySection
  let hapticsEnabled: Bool
  @Environment(\.accessibilityReduceMotion) private var reduceMotion
  @Environment(\.locale) private var locale
  private var expanded: Bool { tab == 4 }
  private var primary: [DockItem] {
    expanded ? [DockItem(id: 3, title: "词典", symbol: "character.book.closed")] : [
      DockItem(id: 0, title: "概览", symbol: "chart.bar.xaxis"),
      DockItem(id: 1, title: "历史", symbol: "clock"),
      DockItem(id: 3, title: "词典", symbol: "character.book.closed"),
      DockItem(id: 2, title: "设置", symbol: "slider.horizontal.3")]
  }
  private var styles: [DockItem] {
    expanded ? StyleLibrarySection.allCases.enumerated().map {
      DockItem(id: $0.offset, title: $0.element.title, symbol: $0.element.symbol)
    } : [DockItem(id: 4, title: "风格", symbol: "sparkles")]
  }
  var body: some View {
    NativeDockPair(primary: primary, styles: styles, primarySelection: expanded ? nil : tab,
      styleSelection: expanded ? StyleLibrarySection.allCases.firstIndex(of: section) : nil,
      expanded: expanded, reduceMotion: reduceMotion, locale: locale,
      onPrimary: { id in
        guard tab != id else { return }
        if expanded { InterfaceFeedback.expansionChanged(enabled: hapticsEnabled) }
        else { InterfaceFeedback.selectionChanged(enabled: hapticsEnabled) }
        tab = id
      }, onStyle: { id in
        let nextSection = StyleLibrarySection.allCases.indices.contains(id)
          ? StyleLibrarySection.allCases[id] : section
        guard !expanded || section != nextSection else { return }
        if expanded { InterfaceFeedback.selectionChanged(enabled: hapticsEnabled) }
        else { InterfaceFeedback.expansionChanged(enabled: hapticsEnabled) }
        section = nextSection
        tab = 4
      })
      .frame(height: 62)
      .transaction { $0.animation = nil }
  }
}

struct DockItem: Equatable {
  let id: Int
  let title: String
  let symbol: String
}

private struct NativeDockPair: UIViewRepresentable {
  let primary: [DockItem]
  let styles: [DockItem]
  let primarySelection: Int?
  let styleSelection: Int?
  let expanded: Bool
  let reduceMotion: Bool
  let locale: Locale
  let onPrimary: (Int) -> Void
  let onStyle: (Int) -> Void
  func makeCoordinator() -> Coordinator { Coordinator() }
  func makeUIView(context: Context) -> DockPairHost {
    let host = DockPairHost()
    host.primary.bar.delegate = context.coordinator
    host.styles.bar.delegate = context.coordinator
    context.coordinator.primary = host.primary.bar
    host.primary.onItemSelect = { [weak coordinator = context.coordinator] in coordinator?.onPrimary?($0) }
    host.styles.onItemSelect = { [weak coordinator = context.coordinator] in coordinator?.onStyle?($0) }
    return host
  }
  func sizeThatFits(_ proposal: ProposedViewSize, uiView: DockPairHost, context: Context) -> CGSize? {
    CGSize(width: proposal.width ?? 370, height: 62)
  }
  func updateUIView(_ host: DockPairHost, context: Context) {
    context.coordinator.onPrimary = onPrimary
    context.coordinator.onStyle = onStyle
    let language = locale.language.languageCode?.identifier ?? "zh"
    host.update(primary: primary, styles: styles, primarySelection: primarySelection,
      styleSelection: styleSelection, expanded: expanded, language: language, reduceMotion: reduceMotion)
  }
  final class Coordinator: NSObject, UITabBarDelegate {
    weak var primary: UITabBar?
    var onPrimary: ((Int) -> Void)?
    var onStyle: ((Int) -> Void)?
    func tabBar(_ tabBar: UITabBar, didSelect item: UITabBarItem) {
      if tabBar === primary { onPrimary?(item.tag) } else { onStyle?(item.tag) }
    }
  }
}

final class DockPairHost: UIView {
  let primary = DockBarHost(identifier: "primaryDock")
  let styles = DockBarHost(identifier: "styleDock")
  private var expanded = false
  private var laidOut = false
  private var animator: UIViewPropertyAnimator?
  private var transitionID = UUID()
  private var configuration: Configuration?
  private struct Configuration: Equatable {
    let primary: [DockItem]
    let styles: [DockItem]
    let primarySelection: Int?
    let styleSelection: Int?
    let expanded: Bool
    let language: String
    let reduceMotion: Bool
  }
  override init(frame: CGRect) {
    super.init(frame: frame)
    addSubview(primary); addSubview(styles)
  }
  required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }
  func update(primary items: [DockItem], styles styleItems: [DockItem], primarySelection: Int?,
    styleSelection: Int?, expanded: Bool, language: String, reduceMotion: Bool) {
    let next = Configuration(primary: items, styles: styleItems,
      primarySelection: primarySelection, styleSelection: styleSelection,
      expanded: expanded, language: language, reduceMotion: reduceMotion)
    // Unrelated model publications must not restart a running transition.
    guard configuration != next else { return }
    configuration = next
    let configure = { [weak self] in
      guard let self else { return }
      _ = self.primary.configure(items: items, selection: primarySelection, language: language)
      _ = self.styles.configure(items: styleItems, selection: styleSelection, language: language)
    }
    let changesGroup = self.expanded != expanded
    let wasAnimating = animator?.state == .active
    let id = UUID(); transitionID = id
    if let animator, animator.state == .active {
      // Sample before stopping: stopping first discards the presentation layers.
      let restorePrimary = primary.capturePresentation()
      let restoreStyles = styles.capturePresentation()
      animator.stopAnimation(true)
      UIView.performWithoutAnimation { restorePrimary(); restoreStyles() }
    }
    animator = nil
    let animated = (changesGroup || wasAnimating) && laidOut && window != nil && !reduceMotion
    if animated {
      primary.beginTransition(); styles.beginTransition()
      self.expanded = expanded
      UIView.performWithoutAnimation {
        configure()
        self.setNeedsLayout(); self.layoutIfNeeded()
        self.primary.prepareIncomingItems(); self.styles.prepareIncomingItems()
      }
      let transition = UIViewPropertyAnimator(duration: 0.28, dampingRatio: 1) { [weak self] in
        guard let self else { return }
        self.primary.animateTransition(); self.styles.animateTransition()
      }
      animator = transition
      transition.addCompletion { [weak self] _ in
        guard let self, self.transitionID == id else { return }
        // Native glass and item snapshots share the same final layout. Keep both
        // alive for the handoff instead of replacing them in a single frame.
        self.primary.prepareHandoff(); self.styles.prepareHandoff()
        let handoff = UIViewPropertyAnimator(duration: 0.12, curve: .easeOut) {
          self.primary.animateHandoff(); self.styles.animateHandoff()
        }
        self.animator = handoff
        handoff.addCompletion { [weak self] _ in
          guard let self, self.transitionID == id else { return }
          self.animator = nil
          UIView.performWithoutAnimation {
            self.primary.finishTransition(); self.styles.finishTransition()
            self.setNeedsLayout(); self.layoutIfNeeded()
          }
        }
        handoff.startAnimation()
      }
      transition.startAnimation()
    } else {
      self.expanded = expanded
      UIView.performWithoutAnimation {
        configure()
        setNeedsLayout(); layoutIfNeeded()
        primary.finishTransition(); styles.finishTransition()
      }
    }
  }
  override func layoutSubviews() {
    super.layoutSubviews()
    guard bounds.width > 0 else { return }
    let circle: CGFloat = 62, gap: CGFloat = 10
    let wide = max(circle, bounds.width - circle - gap)
    let leftWidth = expanded ? circle : wide
    let rightWidth = expanded ? wide : circle
    // Stable parents prevent a second bounds animation from scaling their bars.
    primary.frame = bounds; styles.frame = bounds
    primary.platterFrame = CGRect(x: 0, y: 0, width: leftWidth, height: circle)
    styles.platterFrame = CGRect(x: leftWidth + gap, y: 0, width: rightWidth, height: circle)
    primary.setNeedsLayout(); styles.setNeedsLayout()
    primary.layoutIfNeeded(); styles.layoutIfNeeded()
    laidOut = true
  }
}

final class DockBarHost: UIView {
  let bar = UITabBar()
  var platterFrame = CGRect.zero
  private var gutter: CGFloat = 0
  // UIVisualEffectView can insert an internal hosting layer on newer iOS.
  // A plain wrapper gives presentation sampling the same coordinates as hit testing.
  private let transitionGlass = UIView()
  private let transitionSelection = UIView()
  private var glassEffect = DockBarHost.makeGlassEffect()
  private var transitioning = false
  private var outgoing: [DockMorphItem] = []
  private var incoming: [DockMorphItem] = []
  private var startFrame = CGRect.zero
  private var destinationFrame = CGRect.zero
  private var destinationSelectionFrame = CGRect.zero
  var onItemSelect: ((Int) -> Void)?
  private let identifier: String
  private var items: [DockItem] = []
  private var language = ""
  private var cached: [Int: UITabBarItem] = [:]
  private var pendingItems: [UITabBarItem]?
  private var selectedID: Int?
  init(identifier: String) {
    self.identifier = identifier
    super.init(frame: .zero)
    bar.accessibilityIdentifier = identifier
    transitionGlass.accessibilityIdentifier = identifier + "-transitionGlass"
    bar.itemPositioning = .fill; bar.itemSpacing = 0
    bar.tintColor = UIColor(AppDesign.accent)
    addSubview(bar)
    glassEffect.frame = transitionGlass.bounds
    glassEffect.autoresizingMask = [.flexibleWidth, .flexibleHeight]
    transitionGlass.addSubview(glassEffect)
    transitionGlass.isHidden = true
    addSubview(transitionGlass)
    transitionSelection.accessibilityIdentifier = identifier + "-transitionSelection"
    transitionSelection.backgroundColor = .secondarySystemFill
    transitionSelection.layer.cornerCurve = .continuous
    transitionSelection.isUserInteractionEnabled = false
    transitionSelection.isHidden = true
    addSubview(transitionSelection)
  }
  private static func makeGlassEffect() -> UIVisualEffectView {
    let view: UIVisualEffectView
    if #available(iOS 26, *) {
      let effect = UIGlassEffect(style: .regular)
      effect.isInteractive = true
      view = UIVisualEffectView(effect: effect)
      view.cornerConfiguration = .capsule()
    } else { view = UIVisualEffectView(effect: UIBlurEffect(style: .systemMaterial)) }
    view.layer.cornerRadius = 31
    view.layer.cornerCurve = .continuous
    view.clipsToBounds = true
    return view
  }
  required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }
  func configure(items: [DockItem], selection: Int?, language: String) -> Bool {
    let selectionChanged = selectedID != selection
    selectedID = selection
    guard self.items != items || self.language != language else {
      if selectionChanged { setNeedsLayout() }
      return selectionChanged
    }
    self.items = items; self.language = language
    let bundle = Bundle.main.path(forResource: language == "en" ? "en" : "zh-Hans", ofType: "lproj")
      .flatMap(Bundle.init(path:)) ?? .main
    pendingItems = items.map { item in
      let value = cached[item.id] ?? UITabBarItem(title: nil, image: UIImage(systemName: item.symbol), tag: item.id)
      cached[item.id] = value
      value.title = bundle.localizedString(forKey: item.title, value: item.title, table: nil)
      value.accessibilityIdentifier = identifier == "primaryDock"
        ? (items.count == 1 ? "dockDictionary" : "dockTab-\(item.id)")
        : (items.count == 1 ? "dockStyles" : "dockStyle-" + StyleLibrarySection.allCases[item.id].rawValue)
      return value
    }
    setNeedsLayout()
    return true
  }
  override func layoutSubviews() {
    super.layoutSubviews()
    let fitted = bar.sizeThatFits(platterFrame.size)
    gutter = max(0, fitted.height - platterFrame.height)
    bar.frame = CGRect(x: platterFrame.minX - gutter, y: platterFrame.minY,
      width: platterFrame.width + 2 * gutter, height: fitted.height)
    commitItems()
    if !transitioning { transitionGlass.frame = nativeLayout().frame }
  }

  /// Capture all visible layers, including a partially completed cross-fade.
  /// The returned closure is applied only after the old animator is cancelled.
  func capturePresentation() -> () -> Void {
    let frame = transitionGlass.layer.presentation()?.frame ?? transitionGlass.frame
    let glassAlpha = CGFloat(transitionGlass.layer.presentation()?.opacity ?? Float(transitionGlass.alpha))
    let barAlpha = CGFloat(bar.layer.presentation()?.opacity ?? Float(bar.alpha))
    let selectionFrame = transitionSelection.layer.presentation()?.frame ?? transitionSelection.frame
    let selectionAlpha = CGFloat(transitionSelection.layer.presentation()?.opacity ?? Float(transitionSelection.alpha))
    let restoreItems = (outgoing + incoming).map { $0.capturePresentation() }
    return { [self] in
      transitionGlass.layer.removeAllAnimations(); bar.layer.removeAllAnimations()
      transitionSelection.layer.removeAllAnimations()
      transitionGlass.frame = frame; transitionGlass.alpha = glassAlpha; bar.alpha = barAlpha
      transitionSelection.frame = selectionFrame; transitionSelection.alpha = selectionAlpha
      restoreItems.forEach { $0() }
    }
  }

  func beginTransition() {
    if transitioning {
      startFrame = transitionGlass.frame
      outgoing += incoming
      incoming = []
      // A reversal can arrive during the native handoff. Preserve its visible
      // contribution rather than resurrecting the already faded morph items.
      if !bar.isHidden, bar.alpha > 0 {
        let nativeItems = makeMorphItems()
        nativeItems.forEach { $0.alpha = bar.alpha }
        outgoing += nativeItems
        // UIKit also fades the effect's internal render layers. Cancelling the
        // wrapper animator alone can leave those layers invisible on reversal.
        glassEffect.removeFromSuperview()
        glassEffect = Self.makeGlassEffect()
        glassEffect.frame = transitionGlass.bounds
        glassEffect.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        transitionGlass.addSubview(glassEffect)
      }
    } else {
      let layout = nativeLayout()
      startFrame = layout.frame
      transitionSelection.frame = selectionFrame(in: layout)
      transitionSelection.alpha = selectedID == nil ? 0 : 1
      outgoing = makeMorphItems()
    }
    transitionGlass.frame = startFrame
    transitionGlass.alpha = 1
    transitionGlass.isHidden = false
    transitionSelection.layer.cornerRadius = transitionSelection.bounds.height / 2
    transitionSelection.isHidden = false
    bar.alpha = 0
    bar.isHidden = true
    transitioning = true
  }

  func prepareIncomingItems() {
    let layout = nativeLayout()
    destinationFrame = layout.frame
    destinationSelectionFrame = selectionFrame(in: layout)
    // A newly expanded selection starts with its own platter. It appears during
    // the morph, without waiting for UIKit's selected glass to be unhidden.
    if transitionSelection.alpha == 0 {
      let width = min(max(0, startFrame.width - 6), destinationSelectionFrame.width)
      transitionSelection.frame = CGRect(x: startFrame.midX - width / 2,
        y: startFrame.minY + 4, width: width, height: max(0, startFrame.height - 8))
      transitionSelection.layer.cornerRadius = transitionSelection.bounds.height / 2
    }
    incoming = makeMorphItems(layout: layout)
    for item in incoming {
      item.center = CGPoint(x: startFrame.midX, y: startFrame.midY)
      item.alpha = 0
      item.setBlurred(true)
    }
  }

  func animateTransition() {
    transitionGlass.frame = destinationFrame
    transitionSelection.frame = destinationSelectionFrame
    transitionSelection.layer.cornerRadius = destinationSelectionFrame.height / 2
    transitionSelection.alpha = selectedID == nil ? 0 : 1
    for item in outgoing {
      item.center = CGPoint(x: destinationFrame.midX, y: destinationFrame.midY)
      item.alpha = 0
      item.setBlurred(true)
    }
    for item in incoming {
      item.center = item.destination
      item.alpha = 1
      item.setBlurred(false)
    }
  }

  func prepareHandoff() {
    bar.alpha = 0
    bar.isHidden = false
  }

  func animateHandoff() {
    bar.alpha = 1
    transitionGlass.alpha = 0
    transitionSelection.alpha = 0
    for item in outgoing + incoming { item.alpha = 0 }
  }

  func finishTransition() {
    // Keep a pressed overlay alive through touch-up; it cannot postpone the
    // visual handoff or leave a single icon on an already expanded platter.
    transitioning = false
    transitionGlass.isHidden = true
    transitionGlass.alpha = 1
    transitionSelection.isHidden = true
    bar.isHidden = false
    bar.alpha = 1
    for item in outgoing + incoming {
      if item.isTracking { item.alpha = 0; item.removeAfterTracking = true }
      else { item.removeFromSuperview() }
    }
    outgoing = []; incoming = []
  }

  private struct NativeLayout {
    let frame: CGRect
    let controls: [UIControl]
  }

  private func selectionFrame(in layout: NativeLayout) -> CGRect {
    let index = items.firstIndex { $0.id == selectedID } ?? 0
    let width = layout.frame.width / CGFloat(max(1, items.count))
    return CGRect(x: layout.frame.minX + width * CGFloat(index) + 3,
      y: layout.frame.minY + 4, width: max(0, width - 6),
      height: max(0, layout.frame.height - 8))
  }

  private func nativeLayout() -> NativeLayout {
    // Floating tab bars contain both selected-content and ordinary-content
    // groups. Use one complete sibling group, not their doubled control union.
    // Inspect public UIView geometry only; no private class names or selectors.
    func groups(in parent: UIView) -> [(UIView, [UIControl])] {
      let controls = parent.subviews.compactMap { $0 as? UIControl }.filter {
        !$0.isHidden && $0.bounds.width > 1 && $0.bounds.height > 1
      }
      let own = controls.count == items.count && !controls.isEmpty ? [(parent, controls)] : []
      return own + parent.subviews.flatMap { groups(in: $0) }
    }
    guard let (container, controls) = groups(in: bar).last else {
      return NativeLayout(frame: platterFrame, controls: [])
    }
    var frame = platterFrame
    let candidate = container.convert(container.bounds, to: self)
    if container !== bar, candidate.height > 1,
      candidate.height <= platterFrame.height + 1, candidate.width > 1,
      candidate.width <= bar.bounds.width {
      frame = candidate
    }
    return NativeLayout(frame: frame, controls: controls.sorted {
      $0.convert($0.bounds, to: self).midX < $1.convert($1.bounds, to: self).midX
    })
  }

  private func makeMorphItems(layout: NativeLayout? = nil) -> [DockMorphItem] {
    let layout = layout ?? nativeLayout()
    let bundle = Bundle.main.path(forResource: language == "en" ? "en" : "zh-Hans", ofType: "lproj")
      .flatMap(Bundle.init(path:)) ?? .main
    let width = layout.frame.width / CGFloat(max(1, items.count))
    let native = layout.controls
    return items.enumerated().map { index, item in
      let title = bundle.localizedString(forKey: item.title, value: item.title, table: nil)
      let view: DockMorphItem
      if native.count == items.count {
        let control = native[index]
        let rendered = UIGraphicsImageRenderer(bounds: control.bounds).image { context in
          control.layer.render(in: context.cgContext)
        }
        // Native floating-bar glyphs use vibrancy; detached snapshots otherwise
        // become white on light glass. Keep their exact geometry, resolve ink here.
        let color = item.id == selectedID ? bar.tintColor ?? UIColor.label : UIColor.label
        let image = rendered.withTintColor(color, renderingMode: .alwaysOriginal)
        view = DockMorphItem(image: image, title: title)
        let frame = control.convert(control.bounds, to: self)
        view.destination = CGPoint(x: frame.midX, y: frame.midY)
      } else {
        view = DockMorphItem(title: title, symbol: item.symbol, selected: item.id == selectedID,
          width: width, tint: bar.tintColor)
        view.destination = CGPoint(x: layout.frame.minX + width * (CGFloat(index) + 0.5), y: layout.frame.midY)
      }
      view.accessibilityIdentifier = "dockMorph-\(identifier)-\(item.id)"
      view.center = view.destination
      view.addAction(UIAction { [weak self] _ in self?.onItemSelect?(item.id) }, for: .touchUpInside)
      addSubview(view)
      return view
    }
  }
  func commitItems() {
    let replacingItems = pendingItems != nil
    let layout = {
      if let pendingItems = self.pendingItems {
        self.pendingItems = nil
        self.bar.setItems(pendingItems, animated: false)
      }
      self.bar.setNeedsLayout(); self.bar.layoutIfNeeded()
      let selection = self.bar.items?.first { $0.tag == self.selectedID }
      if self.bar.selectedItem !== selection { self.bar.selectedItem = selection; self.bar.layoutIfNeeded() }
    }
    if replacingItems { UIView.performWithoutAnimation(layout) }
    else { layout() } // The unchanged single item moves with its glass surface.
  }
  override func hitTest(_ point: CGPoint, with event: UIEvent?) -> UIView? {
    if transitioning {
      let visible = transitionGlass.layer.presentation()?.frame ?? transitionGlass.frame
      guard visible.contains(point) else { return nil }
      let candidates = (incoming + outgoing).filter { ($0.layer.presentation()?.opacity ?? Float($0.alpha)) > 0.1 }
      return candidates.min { left, right in
        abs((left.layer.presentation()?.position ?? left.center).x - point.x)
          < abs((right.layer.presentation()?.position ?? right.center).x - point.x)
      }
    }
    let visible = bar.layer.presentation()?.frame ?? bar.frame
    let hitArea = CGRect(x: visible.minX + gutter, y: visible.minY,
      width: max(0, visible.width - 2 * gutter), height: platterFrame.height)
    guard hitArea.contains(point) else { return nil }
    return super.hitTest(point, with: event)
  }
}

/// Only the transition uses rasterized glyphs; native UITabBar owns settled
/// selection, accessibility and press-drag. Blur is precomputed once, never per frame.
private final class DockMorphItem: UIButton {
  private let sharp: UIImageView
  private let blurred: UIImageView
  var destination = CGPoint.zero
  var removeAfterTracking = false
  private static let imageContext = CIContext(options: [.cacheIntermediates: false])

  convenience init(title: String, symbol: String, selected: Bool, width: CGFloat, tint: UIColor) {
    let size = CGSize(width: width, height: 62)
    let image = UIGraphicsImageRenderer(size: size).image { _ in
      let color = selected ? tint : UIColor.label
      let configuration = UIImage.SymbolConfiguration(pointSize: 23, weight: .regular)
      if let icon = UIImage(systemName: symbol, withConfiguration: configuration)?.withTintColor(color, renderingMode: .alwaysOriginal) {
        icon.draw(at: CGPoint(x: (width - icon.size.width) / 2, y: 10))
      }
      let attributes: [NSAttributedString.Key: Any] = [.font: UIFont.systemFont(ofSize: 10, weight: selected ? .semibold : .medium), .foregroundColor: color]
      let text = title as NSString
      let textSize = text.size(withAttributes: attributes)
      text.draw(at: CGPoint(x: (width - textSize.width) / 2, y: 39), withAttributes: attributes)
    }
    self.init(image: image, title: title)
  }

  init(image: UIImage, title: String) {
    sharp = UIImageView(image: image)
    var soft = image
    if let source = CIImage(image: image),
      let output = Self.imageContext.createCGImage(source.applyingFilter("CIGaussianBlur", parameters: [kCIInputRadiusKey: 3 * image.scale]), from: source.extent) {
      soft = UIImage(cgImage: output, scale: image.scale, orientation: .up)
    }
    blurred = UIImageView(image: soft)
    super.init(frame: CGRect(origin: .zero, size: image.size))
    sharp.frame = bounds; blurred.frame = bounds
    addSubview(sharp); addSubview(blurred)
    blurred.alpha = 0
    accessibilityLabel = title
  }
  required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }
  func setBlurred(_ value: Bool) { sharp.alpha = value ? 0 : 1; blurred.alpha = value ? 1 : 0 }
  func capturePresentation() -> () -> Void {
    let position = layer.presentation()?.position ?? center
    let opacity = CGFloat(layer.presentation()?.opacity ?? Float(alpha))
    let sharpAlpha = CGFloat(sharp.layer.presentation()?.opacity ?? Float(sharp.alpha))
    let blurredAlpha = CGFloat(blurred.layer.presentation()?.opacity ?? Float(blurred.alpha))
    return { [self] in
      layer.removeAllAnimations(); sharp.layer.removeAllAnimations(); blurred.layer.removeAllAnimations()
      center = position; alpha = opacity
      sharp.alpha = sharpAlpha; blurred.alpha = blurredAlpha
    }
  }
  override func endTracking(_ touch: UITouch?, with event: UIEvent?) {
    super.endTracking(touch, with: event)
    if removeAfterTracking { removeFromSuperview() }
  }
  override func cancelTracking(with event: UIEvent?) {
    super.cancelTracking(with: event)
    if removeAfterTracking { removeFromSuperview() }
  }
}
