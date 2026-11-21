import OpenLessShared
import UIKit
import XCTest
@testable import OpenLess

nonisolated final class SpaceVoiceOverlayTests: XCTestCase {
  @MainActor func testRecordingStopsIntoOneCancellableProcessingSurface() throws {
    let overlay = SpaceVoiceOverlay(frame: CGRect(x: 0, y: 0, width: 390, height: 263))
    var action: SpaceVoiceGesture.Action?
    overlay.onAction = { action = $0 }
    overlay.begin(canTranslate: true, canSend: true, language: .simplifiedChinese)
    let status = try XCTUnwrap(overlay.subviews.first { $0.accessibilityIdentifier == "spaceVoice.status" } as? UILabel)
    overlay.update(presentation: .recording, level: 0.03)
    XCTAssertEqual(status.text, "正在聆听")
    XCTAssertEqual(overlay.backgroundColor, .clear)
    overlay.update(presentation: .processing, level: 0.03)
    XCTAssertEqual(status.text, "thinking")
    XCTAssertFalse(overlay.gesture.active)
    XCTAssertTrue(overlay.subviews.first { $0.accessibilityIdentifier == "spaceVoice.translate" }!.isHidden)
    let cancel = try XCTUnwrap(overlay.subviews.first { $0.accessibilityIdentifier == "spaceVoice.cancelProcessing" } as? UIButton)
    XCTAssertFalse(cancel.isHidden)
    cancel.sendActions(for: .touchUpInside)
    XCTAssertEqual(action, .cancel)
    overlay.update(presentation: .idle, level: 0)
    XCTAssertTrue(overlay.isHidden)
  }
  @MainActor func testFixedOutputRemainsVisibleAndDoesNotChangeReleaseTarget() throws {
    let overlay = SpaceVoiceOverlay(frame: CGRect(x: 0, y: 0, width: 320, height: 180))
    overlay.begin(canTranslate: true, canSend: true, language: .simplifiedChinese,
      fixedOutput: true, outputTitle: "English")
    let button = try XCTUnwrap(overlay.subviews.first { $0.accessibilityIdentifier == "spaceVoice.translate" } as? UIButton)
    XCTAssertEqual(button.configuration?.subtitle, "English")
    XCTAssertEqual(overlay.release(at: button.center), .translate)
    overlay.frame.size.height = 144
    overlay.begin(canTranslate: true, canSend: false, language: .simplifiedChinese,
      fixedOutput: true, outputTitle: "Portuguese (Brazil)")
    XCTAssertGreaterThanOrEqual(button.frame.height, 44)
    XCTAssertLessThanOrEqual(button.frame.maxY, 110)
    XCTAssertEqual(button.configuration?.subtitleLineBreakMode, .byTruncatingTail)
    overlay.begin(canTranslate: true, canSend: false, language: .simplifiedChinese)
    XCTAssertNil(button.configuration?.subtitle)
    XCTAssertEqual(overlay.release(at: button.center), .translate)
  }
  @MainActor func testThreeTargetsAndTerminalHitTesting() async throws {
    let overlay = SpaceVoiceOverlay(frame: CGRect(x: 0, y: 0, width: 390, height: 208))
    overlay.begin(canTranslate: true, canSend: true, language: .simplifiedChinese)
    let cancel = try XCTUnwrap(overlay.subviews.first { $0.accessibilityIdentifier == "spaceVoice.cancel" })
    let translate = try XCTUnwrap(overlay.subviews.first { $0.accessibilityIdentifier == "spaceVoice.translate" })
    let send = try XCTUnwrap(overlay.subviews.first { $0.accessibilityIdentifier == "spaceVoice.send" })
    XCTAssertLessThan(cancel.center.x, translate.center.x)
    XCTAssertLessThan(translate.center.x, send.center.x)
    _ = overlay.move(to: cancel.center)
    XCTAssertEqual(overlay.gesture.selection, .cancel)
    _ = overlay.move(to: send.center)
    XCTAssertEqual(overlay.gesture.selection, .send)
    XCTAssertEqual(overlay.release(at: translate.center), .translate)
    XCTAssertNil(overlay.release(at: send.center))
    overlay.frame.size = CGSize(width: 320, height: 144)
    overlay.setNeedsLayout(); overlay.layoutIfNeeded()
    XCTAssertGreaterThanOrEqual(translate.frame.minY, 49)
    XCTAssertLessThanOrEqual(send.frame.maxY, 110)
    XCTAssertGreaterThanOrEqual(send.frame.height, 44)
    overlay.frame.size = CGSize(width: 390, height: 208)
    overlay.begin(canTranslate: false, canSend: false, language: .simplifiedChinese)
    XCTAssertEqual((send as? UIButton)?.configuration?.title, "完成输入")
    XCTAssertEqual(overlay.release(at: CGPoint(x: 195, y: 205)), .input)
    overlay.begin(canTranslate: true, canSend: true, language: .simplifiedChinese)
    XCTAssertEqual(overlay.release(at: CGPoint(x: -10, y: 100)), .cancel)
  }

  @MainActor func testReferenceVisualStates() async throws {
    guard let scene = UIApplication.shared.connectedScenes.first as? UIWindowScene else { throw XCTSkip("No scene") }
    let previous = scene.windows.first(where: \.isKeyWindow)
    let window = UIWindow(windowScene: scene)
    let controller = UIViewController(); window.rootViewController = controller
    controller.view.backgroundColor = .systemBackground
    let overlay = SpaceVoiceOverlay()
    overlay.translatesAutoresizingMaskIntoConstraints = false
    controller.view.addSubview(overlay)
    NSLayoutConstraint.activate([
      overlay.leadingAnchor.constraint(equalTo: controller.view.leadingAnchor),
      overlay.trailingAnchor.constraint(equalTo: controller.view.trailingAnchor),
      overlay.bottomAnchor.constraint(equalTo: controller.view.safeAreaLayoutGuide.bottomAnchor),
      overlay.heightAnchor.constraint(equalToConstant: 263),
    ])
    window.makeKeyAndVisible()
    defer { window.isHidden = true; previous?.makeKey() }
    for style in [UIUserInterfaceStyle.light, .dark] {
      window.overrideUserInterfaceStyle = style
      controller.view.layoutIfNeeded()
      overlay.begin(canTranslate: true, canSend: true, language: .simplifiedChinese,
        fixedOutput: true, outputTitle: "English")
      overlay.update(presentation: .recording, level: 0.035)
      for id in ["cancel", "translate", "send"] {
        let control = try XCTUnwrap(overlay.subviews.first { $0.accessibilityIdentifier == "spaceVoice." + id })
        _ = overlay.move(to: control.center)
        try await Task.sleep(for: .milliseconds(180))
        let picture = UIGraphicsImageRenderer(bounds: overlay.bounds).image { _ in
          overlay.drawHierarchy(in: overlay.bounds, afterScreenUpdates: true)
        }
        let attachment = XCTAttachment(image: picture)
        attachment.name = "space-\(style.rawValue)-\(id)"
        attachment.lifetime = .keepAlways; add(attachment)
      }
      overlay.update(presentation: .processing, level: 0)
      controller.view.layoutIfNeeded()
      let processing = XCTAttachment(image: UIGraphicsImageRenderer(bounds: overlay.bounds).image { _ in
        overlay.drawHierarchy(in: overlay.bounds, afterScreenUpdates: true)
      })
      processing.name = "space-\(style.rawValue)-processing"
      processing.lifetime = .keepAlways
      add(processing)
    }
  }
}
