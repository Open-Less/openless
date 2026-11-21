import XCTest
import UIKit
@testable import OpenLess

nonisolated final class DockTransitionTests: XCTestCase {
  @MainActor private var primary: [DockItem] { [DockItem(id: 0, title: "概览", symbol: "chart.bar.xaxis"),
    DockItem(id: 1, title: "历史", symbol: "clock"),
    DockItem(id: 3, title: "词典", symbol: "character.book.closed"),
    DockItem(id: 2, title: "设置", symbol: "slider.horizontal.3")] }
  @MainActor private var styles: [DockItem] { [DockItem(id: 0, title: "我的风格", symbol: "sparkles"),
    DockItem(id: 1, title: "风格市场", symbol: "square.grid.2x2"),
    DockItem(id: 2, title: "翻译目标", symbol: "globe")] }

  @MainActor private func update(_ host: DockPairHost, expanded: Bool,
    selection: Int = 0, reduceMotion: Bool = false) {
    host.update(primary: expanded ? [primary[2]] : primary,
      styles: expanded ? styles : [DockItem(id: 4, title: "风格", symbol: "sparkles")],
      primarySelection: expanded ? nil : 0, styleSelection: expanded ? selection : nil,
      expanded: expanded, language: "zh", reduceMotion: reduceMotion)
  }

  @MainActor func testAllDestinationIconsAppearDuringExpansionAndReversal() async throws {
    let scene = try XCTUnwrap(UIApplication.shared.connectedScenes.first as? UIWindowScene)
    let window = UIWindow(windowScene: scene)
    let controller = UIViewController()
    controller.view.backgroundColor = .systemBackground
    window.rootViewController = controller
    window.isHidden = false
    defer {
      window.isHidden = true
      window.rootViewController = nil
      window.windowScene = nil
    }
    let host = DockPairHost(frame: CGRect(x: 16, y: 150, width: 370, height: 62))
    controller.view.addSubview(host)
    update(host, expanded: false)
    host.layoutIfNeeded()
    capture(controller.view, name: "dock-before")
    update(host, expanded: true)
    try await Task.sleep(for: .milliseconds(160))
    let incoming = host.styles.subviews.compactMap { $0 as? UIButton }.filter {
      ["dockMorph-styleDock-0", "dockMorph-styleDock-1", "dockMorph-styleDock-2"].contains($0.accessibilityIdentifier ?? "")
    }
    XCTAssertEqual(incoming.count, 3)
    for item in incoming {
      XCTAssertGreaterThan(item.layer.presentation()?.opacity ?? 0, 0.2)
      XCTAssertEqual(item.transform.a, 1, accuracy: 0.001, "Glyph size must remain constant")
    }
    let positions = incoming.compactMap { $0.layer.presentation()?.position.x }.sorted()
    XCTAssertEqual(positions.count, 3)
    if positions.count == 3 { XCTAssertGreaterThan(positions[2] - positions[0], 80) }
    let selection = try XCTUnwrap(host.styles.subviews.first {
      $0.accessibilityIdentifier == "styleDock-transitionSelection"
    })
    XCTAssertFalse(selection.isHidden)
    XCTAssertGreaterThan(selection.layer.presentation()?.opacity ?? 0, 0.2,
      "The selected surface must accompany incoming items, before the native bar handoff")
    capture(controller.view, name: "dock-expanding-160ms")
    update(host, expanded: false)
    try await Task.sleep(for: .milliseconds(160))
    capture(controller.view, name: "dock-reversing-160ms")
    try await Task.sleep(for: .milliseconds(500))
    XCTAssertFalse(host.primary.bar.isHidden)
    XCTAssertFalse(host.styles.bar.isHidden)
    XCTAssertEqual(host.primary.bar.items?.count, 4)
    XCTAssertEqual(host.styles.bar.items?.count, 1)
    XCTAssertTrue(host.primary.subviews.compactMap { $0 as? UIButton }.isEmpty)
    capture(controller.view, name: "dock-restored")
    update(host, expanded: true, reduceMotion: true)
    XCTAssertFalse(host.styles.bar.isHidden)
    XCTAssertEqual(host.styles.bar.items?.count, 3)
  }

  @MainActor func testSelectingAnotherSectionDuringExpansionUpdatesImmediately() async throws {
    let scene = try XCTUnwrap(UIApplication.shared.connectedScenes.first as? UIWindowScene)
    let window = UIWindow(windowScene: scene)
    let controller = UIViewController()
    window.rootViewController = controller
    window.isHidden = false
    defer {
      window.isHidden = true
      window.rootViewController = nil
      window.windowScene = nil
    }
    let host = DockPairHost(frame: CGRect(x: 16, y: 150, width: 370, height: 62))
    controller.view.addSubview(host)
    update(host, expanded: false)
    host.layoutIfNeeded()
    update(host, expanded: true)
    try await Task.sleep(for: .milliseconds(80))
    update(host, expanded: true, selection: 2)
    XCTAssertEqual(host.styles.bar.selectedItem?.tag, 2,
      "A new selection must not wait for the previous expansion and handoff to finish")
    try await Task.sleep(for: .milliseconds(160))
    capture(controller.view, name: "dock-selection-retargeted-during-expansion")
    update(host, expanded: false, reduceMotion: true)
    XCTAssertFalse(host.primary.bar.isHidden)
    XCTAssertEqual(host.primary.bar.items?.count, 4)
    XCTAssertEqual(host.primary.bar.selectedItem?.tag, 0)
    update(host, expanded: true, selection: 2, reduceMotion: true)
    XCTAssertEqual(host.styles.bar.selectedItem?.tag, 2)
    XCTAssertFalse(host.styles.bar.isHidden)
    XCTAssertTrue(host.styles.subviews.compactMap { $0 as? UIButton }.isEmpty,
      "Reduced motion must complete cleanup after an interrupted selection")
  }

  @MainActor private func capture(_ view: UIView, name: String) {
    let image = UIGraphicsImageRenderer(bounds: CGRect(x: 0, y: 130, width: 402, height: 110)).image { _ in
      view.drawHierarchy(in: view.bounds, afterScreenUpdates: false)
    }
    let attachment = XCTAttachment(image: image)
    attachment.name = name
    attachment.lifetime = .keepAlways
    add(attachment)
  }
}
