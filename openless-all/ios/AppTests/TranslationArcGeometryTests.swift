import XCTest
import UIKit
import OpenLessShared

/// Measures the rendered UIView and UILabel geometry on the main actor.
/// Geometry uses the production view; only the irrelevant canvas color is supplied here.
@MainActor
enum TranslationArcGeometryChecks {
  static func run() {
    defer { hosts.removeAll() }
    for width in [CGFloat(320), 402, 844] {
      for count in 1...3 {
        let arc = hosted(width: width, category: .large)
        arc.begin(targets: options(count))
        let cancel = button("translationCancel", in: arc)
        let cancelFrame = cancel.convert(cancel.bounds, to: arc)
        XCTAssertTrue(cancel.bounds.height >= 44 && cancel.bounds.width >= 44)
        XCTAssertTrue(arc.bounds.contains(cancelFrame), "Regular cancel must fit the initial 200pt viewport")
        for i in 0..<count {
          let target = button("translationTarget.t\(i)", in: arc)
          let frame = target.convert(target.bounds, to: arc)
          XCTAssertTrue(target.bounds.height >= 44 && target.bounds.width >= 44)
          XCTAssertTrue(arc.bounds.contains(frame))
          XCTAssertTrue(frame.maxY < cancelFrame.minY)
        }
        let first = button("translationTarget.t0", in: arc)
        arc.move(to: first.convert(CGPoint(x: first.bounds.midX, y: first.bounds.midY), to: arc))
        XCTAssertTrue(arc.gesture.selectedID == "t0")
        XCTAssertTrue(arc.release(at: CGPoint(x: cancelFrame.midX, y: cancelFrame.midY)) == nil,
          "The terminal cancel point must override the earlier target")
      }
    }
    let paged = hosted(width: 320, category: .large)
    paged.begin(targets: options(7))
    button("translationNextPage", in: paged).sendActions(for: .touchUpInside)
    let target = button("translationTarget.t3", in: paged)
    XCTAssertTrue(paged.release(at: target.convert(CGPoint(x: target.bounds.midX, y: target.bounds.midY), to: paged)) == "t3")
    let large = hosted(width: 320, category: .accessibilityExtraExtraExtraLarge)
    large.begin(targets: options(3))
    let scroll = large.subviews.compactMap { $0 as? UIScrollView }.first!
    let cancel = button("translationCancel", in: large)
    XCTAssertTrue(scroll.contentSize.height > scroll.bounds.height)
    XCTAssertTrue(cancel.bounds.height >= 44)
    XCTAssertTrue(cancel.frame.maxY <= scroll.contentSize.height)
  }
  private static var hosts: [UIViewController] = []
  private static func hosted(width: CGFloat, category: UIContentSizeCategory) -> TranslationArcView {
    let parent = UIViewController()
    parent.view.frame = CGRect(x: 0, y: 0, width: width, height: 200)
    let child = UIViewController()
    let arc = TranslationArcView(frame: parent.view.bounds)
    child.view = arc
    parent.addChild(child); parent.view.addSubview(arc); child.didMove(toParent: parent)
    parent.setOverrideTraitCollection(UITraitCollection(preferredContentSizeCategory: category), forChild: child)
    if #available(iOS 17.0, *) {
      arc.traitOverrides.preferredContentSizeCategory = category
      arc.updateTraitsIfNeeded()
    }
    XCTAssertEqual(arc.traitCollection.preferredContentSizeCategory, category)
    arc.setNeedsLayout(); arc.layoutIfNeeded()
    hosts.append(parent)
    return arc
  }
  private static func options(_ count: Int) -> [KeyboardVoiceOption] {
    (0..<count).map { KeyboardVoiceOption(id: "t\($0)", title: ["English", "简体中文", "日本語"][$0 % 3]) }
  }
  private static func button(_ id: String, in view: UIView) -> UIButton {
    if let button = view as? UIButton, button.accessibilityIdentifier == id { return button }
    for child in view.subviews {
      if let result = find(id, in: child) { return result }
    }
    XCTFail("Missing actual component button: \(id)")
    return UIButton(type: .system)
  }
  private static func find(_ id: String, in view: UIView) -> UIButton? {
    if let button = view as? UIButton, button.accessibilityIdentifier == id { return button }
    return view.subviews.lazy.compactMap { find(id, in: $0) }.first
  }
}

// The production arc uses KeyboardStyle only for its transparent canvas.
@MainActor enum KeyboardStyle { static let canvas = UIColor.clear }

nonisolated final class TranslationArcGeometryTests: XCTestCase {
  @MainActor func testCompactCancellationAndFinalReleaseStayReachable() {
    TranslationArcGeometryChecks.run()
  }
}
